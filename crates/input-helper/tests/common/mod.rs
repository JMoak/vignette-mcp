//! The built helper as the console meets it: a process speaking JSON lines.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use input_helper::json::{self, Value};

pub const PATIENCE: Duration = Duration::from_secs(5);

pub struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<String>,
}

impl Helper {
    /// The helper started dry: it speaks the whole protocol and injects and watches nothing.
    pub fn dry() -> Helper {
        Helper::start(&["--dry-run"])
    }

    /// The helper as the console starts it, injecting real input. Only for tests that have made
    /// sure this machine allows that.
    pub fn injecting() -> Helper {
        Helper::start(&[])
    }

    fn start(arguments: &[&str]) -> Helper {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kw-input-helper"))
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the helper starts");
        let output = child.stdout.take().expect("its output");
        let (lines, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if lines.send(line).is_err() {
                    return;
                }
            }
        });
        Helper { input: child.stdin.take(), child, replies }
    }

    pub fn greet(&mut self) -> String {
        self.send(r#"{"v":1,"kind":"hello","versions":[1]}"#);
        self.reply().expect("a hello")
    }

    pub fn send(&mut self, line: &str) {
        if let Some(input) = self.input.as_mut() {
            let _ = writeln!(input, "{line}").and_then(|()| input.flush());
        }
    }

    pub fn reply(&self) -> Option<String> {
        self.reply_within(PATIENCE)
    }

    pub fn reply_within(&self, wait: Duration) -> Option<String> {
        self.replies.recv_timeout(wait).ok()
    }

    /// Replies up to and including the first one `wanted` picks, or all there are until it stops.
    pub fn replies_until(&self, wanted: impl Fn(&Value) -> bool) -> Vec<String> {
        let mut seen = Vec::new();
        while let Some(line) = self.reply() {
            let done = json::parse(&line).is_some_and(|reply| wanted(&reply));
            seen.push(line);
            if done {
                break;
            }
        }
        seen
    }

    pub fn close_input(&mut self) {
        self.input.take();
    }

    pub fn exit_code(&mut self) -> Option<i32> {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status.code();
            }
            thread::sleep(Duration::from_millis(5));
        }
        None
    }

    /// What the console's port does to stop it: release all, bye, close its input.
    pub fn stop(mut self) {
        self.send(r#"{"v":1,"kind":"release-all"}"#);
        self.send(r#"{"v":1,"kind":"bye"}"#);
        self.close_input();
        assert_eq!(self.exit_code(), Some(0));
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn kind(reply: &Value) -> Option<&str> {
    match reply.field("kind") {
        Some(Value::Text(kind)) => Some(kind),
        _ => None,
    }
}

pub fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_secs_f64() * 1000.0
}

/// A grant line for `display` (the console's bounds and scale, then optionally its pixels).
pub fn grant(lifetime_ms: f64, display: &str) -> String {
    let expires_at = (now_ms() + lifetime_ms).round();
    format!(r#"{{"v":1,"kind":"grant","grantId":"g1","expiresAt":{expires_at},"display":{display}}}"#)
}

pub const DRY_DISPLAY: &str = r#"{"x":0,"y":0,"width":1920,"height":1080,"scaleFactor":1}"#;
