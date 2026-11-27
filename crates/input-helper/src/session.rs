//! One run of the helper: the console greets it, grants it one display for a while, sends input
//! under that grant and finally lets it go. Everything the helper decides is decided here; the
//! backend only carries it out.

use std::collections::{BTreeMap, BTreeSet};

use crate::backend::{Act, Backend, Fault};
use crate::keys::{self, Key};
use crate::limits::{self, Bucket};
use crate::protocol::{self, Action, Button, Display, Rect, Refusal, Reply, Request, VERSION};
use crate::screen;

/// While the console has not sent input since the last report, physical input is reported at
/// most this often: the console has already paused, and a mouse reports up to 1000 times a second.
pub const LOCAL_INPUT_QUIET_MS: f64 = 250.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    Exit(i32),
}

struct Live {
    id: String,
    expires_at: f64,
    display: Display,
    area: Rect,
}

pub struct Session<B> {
    backend: B,
    implementation: &'static str,
    greeted: bool,
    grant: Option<Live>,
    keys: BTreeMap<&'static str, &'static Key>,
    buttons: BTreeSet<Button>,
    motion: Bucket,
    presses: Bucket,
    last_report: Option<f64>,
    sent_since_report: bool,
    finished: bool,
}

impl<B: Backend> Session<B> {
    pub fn new(backend: B, implementation: &'static str) -> Self {
        Session {
            backend,
            implementation,
            greeted: false,
            grant: None,
            keys: BTreeMap::new(),
            buttons: BTreeSet::new(),
            motion: Bucket::new(limits::MOTION_BURST, limits::MOTION_PER_SECOND),
            presses: Bucket::new(limits::PRESSES_BURST, limits::PRESSES_PER_SECOND),
            last_report: None,
            sent_since_report: false,
            finished: false,
        }
    }

    #[cfg(test)]
    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }

    /// When the grant runs out, in the console's milliseconds since the epoch.
    pub fn deadline(&self) -> Option<f64> {
        self.grant.as_ref().map(|grant| grant.expires_at)
    }

    /// One line from the console; `None` for a line that could not be read as text.
    pub fn receive(&mut self, line: Option<&str>, now: f64, out: &mut Vec<Reply>) -> Flow {
        if self.finished {
            return Flow::Go;
        }
        let Some(request) = line.and_then(protocol::parse) else {
            out.push(Reply::Refused { seq: None, reason: Refusal::Malformed });
            return Flow::Go;
        };
        if !self.greeted && !matches!(request, Request::Hello { .. }) {
            out.push(Reply::Refused { seq: None, reason: Refusal::Version });
            return Flow::Go;
        }
        match request {
            Request::Hello { versions } => return self.greet(&versions, out),
            Request::Grant(grant) => self.hold(grant, out),
            Request::Pointer(pointer) => self.pointer(&pointer, now, out),
            Request::Key(key) => self.key(&key, now, out),
            Request::ReleaseAll => self.release_all(out),
            Request::Bye => return self.finish(0, out),
        }
        Flow::Go
    }

    /// Someone used this machine's own keyboard or mouse.
    pub fn local_input(&mut self, now: f64, out: &mut Vec<Reply>) {
        if self.finished || self.grant.is_none() {
            return;
        }
        let due =
            self.sent_since_report || self.last_report.is_none_or(|last| now - last >= LOCAL_INPUT_QUIET_MS);
        if due {
            self.last_report = Some(now);
            self.sent_since_report = false;
            out.push(Reply::LocalInput);
        }
    }

    /// The grant's time has come, or not yet.
    pub fn tick(&mut self, now: f64, out: &mut Vec<Reply>) -> Flow {
        match self.deadline() {
            Some(deadline) if !self.finished && now >= deadline => self.finish(0, out),
            _ => Flow::Go,
        }
    }

    /// The console closed the helper's input, or the helper is being stopped.
    pub fn closed(&mut self, out: &mut Vec<Reply>) -> Flow {
        self.finish(0, out)
    }

    fn greet(&mut self, versions: &[f64], out: &mut Vec<Reply>) -> Flow {
        if !versions.contains(&f64::from(VERSION)) {
            out.push(Reply::Refused { seq: None, reason: Refusal::Version });
            return self.finish(2, out);
        }
        self.greeted = true;
        out.push(Reply::Hello { implementation: self.implementation });
        Flow::Go
    }

    fn hold(&mut self, grant: protocol::Grant, out: &mut Vec<Reply>) {
        let Some(area) = screen::locate(&grant.display, &self.backend.monitors()) else {
            out.push(Reply::Refused { seq: None, reason: Refusal::Display });
            return;
        };
        if let Err(fault) = self.backend.watch() {
            out.push(Reply::Refused { seq: None, reason: refusal_of(fault) });
            return;
        }
        if self.grant.as_ref().is_some_and(|live| live.id != grant.id) {
            self.release_all(out);
        }
        out.push(Reply::Granted { grant_id: grant.id.clone(), expires_at: grant.expires_at });
        self.grant = Some(Live { id: grant.id, expires_at: grant.expires_at, display: grant.display, area });
    }

    fn refusal(&self, grant_id: &str, now: f64) -> Option<Refusal> {
        match &self.grant {
            None => Some(Refusal::NoGrant),
            Some(live) if live.id != grant_id => Some(Refusal::WrongGrant),
            Some(live) if now >= live.expires_at => Some(Refusal::Expired),
            Some(_) => None,
        }
    }

    fn pointer(&mut self, pointer: &protocol::Pointer, now: f64, out: &mut Vec<Reply>) {
        let seq = Some(pointer.seq);
        if let Some(reason) = self.refusal(&pointer.grant_id, now) {
            out.push(Reply::Refused { seq, reason });
            return;
        }
        let pressing = pointer.action == Action::Down && pointer.button.is_some();
        let within_limits = match pointer.action {
            Action::Move | Action::Wheel => self.motion.take(now),
            Action::Down if pressing => self.presses.take(now),
            Action::Down | Action::Up => true,
        };
        if !within_limits {
            out.push(Reply::Refused { seq, reason: Refusal::Rate });
            return;
        }
        // A press is where a stale idea of the display would do harm, so it looks again.
        let area = if pressing { self.relocate() } else { self.grant.as_ref().map(|live| live.area) };
        let Some(area) = area else {
            out.push(Reply::Refused { seq, reason: Refusal::Display });
            return;
        };
        let mut acts = vec![Act::Move(screen::spot_on(&area, pointer.x, pointer.y))];
        match (pointer.action, pointer.button) {
            (Action::Down, Some(button)) => acts.push(Act::Press(button)),
            (Action::Up, Some(button)) if self.buttons.contains(&button) => acts.push(Act::Release(button)),
            (Action::Wheel, _) => acts.push(Act::Wheel { dx: pointer.dx, dy: pointer.dy }),
            _ => {}
        }
        if let Err(fault) = self.send(&acts) {
            out.push(Reply::Refused { seq, reason: refusal_of(fault) });
            return;
        }
        match (pointer.action, pointer.button) {
            (Action::Down, Some(button)) => self.buttons.insert(button),
            (Action::Up, Some(button)) => self.buttons.remove(&button),
            _ => false,
        };
        out.push(Reply::Applied { seq: pointer.seq });
    }

    fn relocate(&mut self) -> Option<Rect> {
        let display = self.grant.as_ref()?.display;
        let area = screen::locate(&display, &self.backend.monitors())?;
        if let Some(live) = self.grant.as_mut() {
            live.area = area;
        }
        Some(area)
    }

    fn key(&mut self, input: &protocol::Key, now: f64, out: &mut Vec<Reply>) {
        let seq = Some(input.seq);
        if let Some(reason) = self.refusal(&input.grant_id, now) {
            out.push(Reply::Refused { seq, reason });
            return;
        }
        let Some(key) = keys::named(&input.code) else {
            out.push(Reply::Refused { seq, reason: Refusal::Malformed });
            return;
        };
        if input.down && !self.presses.take(now) {
            out.push(Reply::Refused { seq, reason: Refusal::Rate });
            return;
        }
        // Lifting a key the helper is not holding would lift it for whoever is: nothing to send.
        let act = match (input.down, self.keys.contains_key(key.code)) {
            (true, _) => Some(Act::KeyDown(key)),
            (false, true) => Some(Act::KeyUp(key)),
            (false, false) => None,
        };
        if let Some(act) = act {
            if let Err(fault) = self.send(&[act]) {
                out.push(Reply::Refused { seq, reason: refusal_of(fault) });
                return;
            }
            if input.down {
                self.keys.insert(key.code, key);
            } else {
                self.keys.remove(key.code);
            }
        }
        out.push(Reply::Applied { seq: input.seq });
    }

    fn send(&mut self, acts: &[Act]) -> Result<(), Fault> {
        self.backend.send(acts)?;
        self.sent_since_report = true;
        Ok(())
    }

    /// Lets go of every button, then every key, the helper holds, and says which they were.
    /// Always answers, whatever the system makes of the releases.
    fn release_all(&mut self, out: &mut Vec<Reply>) {
        let buttons: Vec<Button> = std::mem::take(&mut self.buttons).into_iter().collect();
        let keys: Vec<&'static Key> = std::mem::take(&mut self.keys).into_values().collect();
        let acts: Vec<Act> =
            buttons.iter().copied().map(Act::Release).chain(keys.iter().copied().map(Act::KeyUp)).collect();
        if !acts.is_empty() {
            let _ = self.backend.send(&acts);
        }
        out.push(Reply::Released { keys: keys.iter().map(|key| key.code).collect(), buttons });
    }

    fn finish(&mut self, code: i32, out: &mut Vec<Reply>) -> Flow {
        if !self.finished {
            self.finished = true;
            self.release_all(out);
            out.push(Reply::Bye);
        }
        Flow::Exit(code)
    }
}

fn refusal_of(fault: Fault) -> Refusal {
    match fault {
        Fault::Access => Refusal::Access,
        Fault::Blocked => Refusal::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{Monitor, Spot};

    const LIVE: f64 = 1_000_000.0;

    #[derive(Default)]
    struct Fake {
        monitors: Vec<Monitor>,
        sent: Vec<Act>,
        watching: bool,
        refuse_watch: bool,
        block: bool,
    }

    impl Backend for Fake {
        fn monitors(&mut self) -> Vec<Monitor> {
            self.monitors.clone()
        }

        fn watch(&mut self) -> Result<(), Fault> {
            if self.refuse_watch {
                return Err(Fault::Access);
            }
            self.watching = true;
            Ok(())
        }

        fn send(&mut self, acts: &[Act]) -> Result<(), Fault> {
            if self.block {
                return Err(Fault::Blocked);
            }
            self.sent.extend_from_slice(acts);
            Ok(())
        }
    }

    fn primary() -> Monitor {
        Monitor { rect: Rect { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 }, scale: Some(1.0) }
    }

    fn session() -> Session<Fake> {
        Session::new(Fake { monitors: vec![primary()], ..Fake::default() }, "fake")
    }

    fn grant_line(id: &str, expires_at: f64) -> String {
        format!(
            r#"{{"v":1,"kind":"grant","grantId":"{id}","expiresAt":{expires_at},"display":{{"x":0,"y":0,"width":1920,"height":1080,"scaleFactor":1}}}}"#
        )
    }

    fn key_line(seq: u64, code: &str, down: bool) -> String {
        format!(r#"{{"v":1,"kind":"key","grantId":"g1","seq":{seq},"code":"{code}","down":{down}}}"#)
    }

    fn ptr_line(seq: u64, action: &str, x: f64, y: f64, button: &str) -> String {
        format!(
            r#"{{"v":1,"kind":"ptr","grantId":"g1","seq":{seq},"action":"{action}","x":{x},"y":{y}{button}}}"#
        )
    }

    struct Run {
        session: Session<Fake>,
        replies: Vec<Reply>,
        now: f64,
    }

    impl Run {
        fn greeted() -> Run {
            let mut run = Run { session: session(), replies: Vec::new(), now: 1000.0 };
            run.send(r#"{"v":1,"kind":"hello","versions":[1]}"#);
            run
        }

        fn granted() -> Run {
            let mut run = Run::greeted();
            run.send(&grant_line("g1", LIVE));
            run.replies.clear();
            run
        }

        fn send(&mut self, line: &str) -> Flow {
            self.session.receive(Some(line), self.now, &mut self.replies)
        }

        fn last(&self) -> Option<&Reply> {
            self.replies.last()
        }

        fn sent(&mut self) -> Vec<Act> {
            std::mem::take(&mut self.session.backend().sent)
        }
    }

    fn key(code: &str) -> &'static Key {
        keys::named(code).expect("a known key")
    }

    #[test]
    fn answers_only_hello_until_greeted_and_leaves_on_a_version_it_does_not_speak() {
        let mut run = Run { session: session(), replies: Vec::new(), now: 1000.0 };
        assert_eq!(run.send(&grant_line("g1", LIVE)), Flow::Go);
        assert_eq!(run.last(), Some(&Reply::Refused { seq: None, reason: Refusal::Version }));
        assert_eq!(run.session.receive(None, 1000.0, &mut run.replies), Flow::Go);
        assert_eq!(run.last(), Some(&Reply::Refused { seq: None, reason: Refusal::Malformed }));
        assert_eq!(run.send(r#"{"v":1,"kind":"hello","versions":[2]}"#), Flow::Exit(2));
        assert_eq!(
            run.replies[2..],
            [
                Reply::Refused { seq: None, reason: Refusal::Version },
                Reply::Released { keys: vec![], buttons: vec![] },
                Reply::Bye
            ]
        );
        assert!(!run.session.backend().watching);
    }

    #[test]
    fn presses_where_the_keeper_points_and_lifts_only_what_it_holds() {
        let mut run = Run::granted();
        run.send(&ptr_line(1, "down", 0.5, 0.25, r#","button":"left""#));
        assert_eq!(run.sent(), [Act::Move(Spot { x: 960.0, y: 270.0 }), Act::Press(Button::Left)]);
        run.send(&ptr_line(2, "up", 1.0, 1.0, r#","button":"left""#));
        assert_eq!(run.sent(), [Act::Move(Spot { x: 1919.0, y: 1079.0 }), Act::Release(Button::Left)]);
        run.send(&ptr_line(3, "up", 0.0, 0.0, r#","button":"right""#));
        assert_eq!(run.sent(), [Act::Move(Spot { x: 0.0, y: 0.0 })]);
        run.send(&key_line(4, "KeyQ", false));
        assert_eq!(run.sent(), []);
        run.send(&ptr_line(5, "wheel", 0.0, 0.0, r#","dx":-4,"dy":120"#));
        assert_eq!(run.sent(), [Act::Move(Spot { x: 0.0, y: 0.0 }), Act::Wheel { dx: -4.0, dy: 120.0 }]);
        let applied: Vec<&Reply> =
            run.replies.iter().filter(|reply| matches!(reply, Reply::Applied { .. })).collect();
        assert_eq!(applied.len(), 5);
    }

    #[test]
    fn release_all_lets_go_of_buttons_then_keys_and_names_them_in_order() {
        let mut run = Run::granted();
        run.send(&key_line(1, "ShiftLeft", true));
        run.send(&key_line(2, "KeyQ", true));
        run.send(&key_line(3, "KeyQ", true));
        run.send(&ptr_line(1, "down", 0.1, 0.1, r#","button":"right""#));
        run.send(&ptr_line(2, "down", 0.1, 0.1, r#","button":"left""#));
        run.sent();
        run.send(r#"{"v":1,"kind":"release-all"}"#);
        assert_eq!(
            run.sent(),
            [
                Act::Release(Button::Left),
                Act::Release(Button::Right),
                Act::KeyUp(key("KeyQ")),
                Act::KeyUp(key("ShiftLeft"))
            ]
        );
        assert_eq!(
            run.last(),
            Some(&Reply::Released {
                keys: vec!["KeyQ", "ShiftLeft"],
                buttons: vec![Button::Left, Button::Right]
            })
        );
        run.send(r#"{"v":1,"kind":"release-all"}"#);
        assert_eq!(run.sent(), []);
        assert_eq!(run.last(), Some(&Reply::Released { keys: vec![], buttons: vec![] }));
    }

    #[test]
    fn lets_go_of_everything_when_the_session_ends_however_it_ends() {
        let endings: [fn(&mut Run) -> Flow; 3] = [
            |run| run.send(r#"{"v":1,"kind":"bye"}"#),
            |run| run.session.closed(&mut run.replies),
            |run| run.session.tick(LIVE, &mut run.replies),
        ];
        for end in endings {
            let mut run = Run::granted();
            run.send(&key_line(1, "AltLeft", true));
            run.sent();
            assert_eq!(run.session.tick(LIVE - 1.0, &mut run.replies), Flow::Go);
            assert_eq!(end(&mut run), Flow::Exit(0));
            assert_eq!(run.sent(), [Act::KeyUp(key("AltLeft"))]);
            assert_eq!(
                run.replies[run.replies.len() - 2..],
                [Reply::Released { keys: vec!["AltLeft"], buttons: vec![] }, Reply::Bye]
            );
            assert_eq!(run.send(&key_line(2, "AltLeft", true)), Flow::Go);
            assert_eq!(run.sent(), []);
        }
    }

    #[test]
    fn a_new_grant_lets_go_of_what_the_old_one_held_and_a_renewal_does_not() {
        let mut run = Run::granted();
        run.send(&key_line(1, "ControlLeft", true));
        run.send(&grant_line("g1", LIVE + 1000.0));
        assert_eq!(run.session.deadline(), Some(LIVE + 1000.0));
        run.sent();
        run.send(&grant_line("g2", LIVE));
        assert_eq!(run.sent(), [Act::KeyUp(key("ControlLeft"))]);
        assert_eq!(
            run.replies[run.replies.len() - 2],
            Reply::Released { keys: vec!["ControlLeft"], buttons: vec![] }
        );
        run.send(&key_line(2, "KeyA", true));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(2), reason: Refusal::WrongGrant }));
    }

    #[test]
    fn refuses_a_grant_for_a_display_it_cannot_find_or_input_it_cannot_watch() {
        let mut run = Run::greeted();
        let elsewhere = grant_line("g1", LIVE).replace(r#""x":0"#, r#""x":1920"#);
        run.send(&elsewhere);
        assert_eq!(run.last(), Some(&Reply::Refused { seq: None, reason: Refusal::Display }));
        run.session.backend().refuse_watch = true;
        run.send(&grant_line("g1", LIVE));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: None, reason: Refusal::Access }));
        run.send(&key_line(1, "KeyA", true));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(1), reason: Refusal::NoGrant }));
        assert_eq!(run.sent(), []);
    }

    #[test]
    fn looks_for_the_display_again_before_a_press_and_refuses_if_it_has_gone() {
        let mut run = Run::granted();
        run.session.backend().monitors = vec![];
        run.send(&ptr_line(1, "move", 0.0, 0.0, ""));
        assert_eq!(run.last(), Some(&Reply::Applied { seq: 1 }));
        run.send(&ptr_line(2, "down", 0.0, 0.0, r#","button":"left""#));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(2), reason: Refusal::Display }));
        assert_eq!(run.sent(), [Act::Move(Spot { x: 0.0, y: 0.0 })]);
    }

    #[test]
    fn input_the_system_refuses_is_not_held() {
        let mut run = Run::granted();
        run.session.backend().block = true;
        run.send(&key_line(1, "ShiftLeft", true));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(1), reason: Refusal::Blocked }));
        run.session.backend().block = false;
        run.send(r#"{"v":1,"kind":"release-all"}"#);
        assert_eq!(run.last(), Some(&Reply::Released { keys: vec![], buttons: vec![] }));
    }

    #[test]
    fn limits_moves_and_presses_but_never_a_release() {
        let mut run = Run::granted();
        let moves = (1..=200).filter(|seq| {
            run.send(&ptr_line(*seq, "move", 0.5, 0.5, ""));
            matches!(run.last(), Some(Reply::Applied { .. }))
        });
        assert_eq!(moves.count(), limits::MOTION_BURST as usize);
        let presses = (1..=40).filter(|seq| {
            run.send(&key_line(*seq, "KeyA", true));
            matches!(run.last(), Some(Reply::Applied { .. }))
        });
        assert_eq!(presses.count(), limits::PRESSES_BURST as usize);
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(40), reason: Refusal::Rate }));
        run.send(&key_line(41, "KeyA", false));
        assert_eq!(run.last(), Some(&Reply::Applied { seq: 41 }));
        assert_eq!(run.sent().last(), Some(&Act::KeyUp(key("KeyA"))));
    }

    #[test]
    fn reports_physical_input_only_under_a_grant_and_at_once_after_input_was_sent() {
        let mut run = Run::greeted();
        run.replies.clear();
        run.session.local_input(1000.0, &mut run.replies);
        assert_eq!(run.replies, []);
        run.send(&grant_line("g1", LIVE));
        let reports = |run: &mut Run, at: &[f64]| {
            run.replies.clear();
            for now in at {
                run.session.local_input(*now, &mut run.replies);
            }
            run.replies.iter().filter(|reply| **reply == Reply::LocalInput).count()
        };
        assert_eq!(reports(&mut run, &[1000.0, 1001.0, 1100.0, 1249.0]), 1);
        assert_eq!(reports(&mut run, &[1250.0, 1300.0]), 1);
        run.now = 1310.0;
        run.send(&ptr_line(1, "move", 0.5, 0.5, ""));
        assert_eq!(reports(&mut run, &[1311.0, 1312.0]), 1);
    }

    #[test]
    fn refuses_a_key_it_does_not_know() {
        let mut run = Run::granted();
        run.send(&key_line(1, "Power", true));
        assert_eq!(run.last(), Some(&Reply::Refused { seq: Some(1), reason: Refusal::Malformed }));
        assert_eq!(run.sent(), []);
    }
}
