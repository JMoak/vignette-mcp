//! Sets out a handful of panes, arranges them three ways and says how it went.

use std::time::{Duration, Instant};

use vignette_core::deal::{self, Side};
use vignette_core::lattice::{Rect, Seat};
use vignette_core::markdown::blocks;
use vignette_core::mood::Mood;
use vignette_host::look::AgentColor;
use vignette_host::pane::Pane;
use vignette_host::stage::{Glide, Stage, Track};

const NOTES: [(&str, Option<&str>, &str); 6] = [
    (
        "Option A",
        Some("draft"),
        "## Ship the larger change\n\n- [x] migration written\n- [ ] rollback rehearsed\n- [ ] docs updated\n\nRisk is higher, payoff is larger.",
    ),
    (
        "Option B",
        Some("chosen"),
        "## Ship the smaller fix first\n\n| step | owner | days |\n|---|---|---|\n| patch | Ana | 1 |\n| verify | Ravi | 1 |\n\nLow risk.",
    ),
    ("Option C", None, "Wait a week and decide with more data."),
    ("Budget", Some("unsure"), "## About 5,700\n\nThe venue quote is the open question."),
    ("Agenda", None, "- arrive\n- plan the quarter\n- dinner"),
    ("Second opinion", None, "I would pick B: it is the one we can undo."),
];

const AGENTS: [&str; 2] = ["claude-code", "codex"];

pub fn run(started: Instant, args: &[String]) -> i32 {
    let count = args.first().and_then(|said| said.parse().ok()).unwrap_or(12).clamp(1, 16);
    let mood = args.get(1).and_then(|said| Mood::read(said)).unwrap_or(Mood::Playful);
    let hold = Duration::from_millis(named(args, "--hold").and_then(|ms| ms.parse().ok()).unwrap_or(1_500));
    let shot = named(args, "--shot");
    let Some(mut stage) = Stage::open() else {
        println!("the stage could not be opened here");
        return 1;
    };
    let gap = (12.0 * stage.scale).round() as i32;
    let room = room_on(stage.area, gap);
    let travel = (96.0 * stage.scale).round() as i32;
    let colours = agent_colours();

    let grid = seated(&deal::grid(count), room, gap);
    let mut opened = Vec::new();
    let mut first = None;
    for (at, to) in grid.iter().enumerate() {
        let (title, mark, words) = NOTES[at % NOTES.len()];
        let agent = at % AGENTS.len();
        let pane = Pane {
            title: if at < NOTES.len() { title.into() } else { format!("{title} {}", at / NOTES.len() + 1) },
            mark: mark.map(str::to_owned),
            by: format!("{} showed a note", AGENTS[agent]),
            agent: Some(colours[agent]),
            blocks: blocks(words),
            scale: stage.scale,
        };
        let Some((_, took)) = stage.show(pane, Rect { x: to.x - travel, ..*to }) else {
            println!("a pane could not be opened");
            return 1;
        };
        opened.push(took);
        first.get_or_insert_with(|| started.elapsed());
    }

    let arrived = stage.glide(&tracks(&grid, mood), false);
    stage.hold(Duration::from_millis(500));
    save(&stage, room, shot, "grid");

    let lead = seated(&deal::lead_and_stack(count, Side::Right), room, gap);
    let led = stage.glide(&tracks(&lead, mood), true);
    stage.hold(Duration::from_millis(500));
    save(&stage, room, shot, "lead");

    let back = stage.glide(&tracks(&grid, Mood::Decisive), true);
    stage.hold(hold);

    let leaving: Vec<Rect> = grid.iter().map(|at| Rect { x: at.x + travel, ..*at }).collect();
    let left = stage.glide(&tracks(&leaving, Mood::Calm), false);
    stage.withdraw_all();

    let slowest = opened.iter().max().copied().unwrap_or_default();
    let mean = opened.iter().sum::<Duration>() / opened.len().max(1) as u32;
    println!("panes: {count}, mood: {}, scale: {:.2}, room: {}x{}", mood.word(), stage.scale, room.w, room.h);
    println!("first pane on screen: {:.1} ms after the program started", ms(first.unwrap_or_default()));
    println!("opening a pane: {:.2} ms mean, {:.2} ms slowest", ms(mean), ms(slowest));
    println!(
        "all {count} on screen: {:.1} ms after the program started",
        ms(first.unwrap_or_default() + opened.iter().skip(1).sum::<Duration>())
    );
    report("arrive (move only)", arrived);
    report("to lead-and-stack (move and resize)", led);
    report("back to grid (move and resize)", back);
    report("leave (move only)", left);
    0
}

fn named<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == flag).and_then(|at| args.get(at + 1)).map(String::as_str)
}

fn room_on(area: Rect, gap: i32) -> Rect {
    let wide = area.w * 62 / 100;
    Rect { x: area.x + area.w - wide - gap, y: area.y + gap, w: wide, h: area.h - 2 * gap }
}

fn seated(seats: &[Seat], room: Rect, gap: i32) -> Vec<Rect> {
    seats.iter().map(|seat| seat.on(room, gap)).collect()
}

fn tracks(to: &[Rect], mood: Mood) -> Vec<Track> {
    let starts = mood.starts_ms(to.len());
    to.iter()
        .enumerate()
        .map(|(pane, to)| Track { pane, to: *to, start_ms: starts[pane], glide_ms: mood.glide_ms() })
        .collect()
}

fn agent_colours() -> Vec<AgentColor> {
    let mut taken = Vec::new();
    for name in AGENTS {
        let colour = AgentColor::kept_for(name, &taken);
        taken.push(colour);
    }
    taken
}

fn save(stage: &Stage, room: Rect, shot: Option<&str>, name: &str) {
    let Some(prefix) = shot else { return };
    let path = format!("{prefix}-{name}.bmp");
    match std::fs::write(&path, stage.picture(room)) {
        Ok(()) => println!("picture: {path}"),
        Err(problem) => println!("picture not saved to {path}: {problem}"),
    }
}

fn report(what: &str, glide: Glide) {
    println!(
        "{what}: {} frames in {:.0} ms, {:.2} ms a frame (display {:.2} ms), worst {:.2} ms, late {}",
        glide.frames,
        glide.took_ms,
        glide.mean_us as f64 / 1_000.0,
        glide.period_us as f64 / 1_000.0,
        glide.worst_us as f64 / 1_000.0,
        glide.late,
    );
}

fn ms(time: Duration) -> f64 {
    time.as_secs_f64() * 1_000.0
}
