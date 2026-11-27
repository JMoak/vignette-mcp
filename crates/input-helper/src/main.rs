use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use input_helper::backend::{Backend, Dry, Event};
use input_helper::lines::Lines;
use input_helper::protocol::{self, Reply};
use input_helper::session::{Flow, Session};

#[cfg(target_os = "macos")]
use input_helper::macos::Mac as System;
#[cfg(windows)]
use input_helper::win32::Win32 as System;
#[cfg(target_os = "linux")]
use input_helper::x11::X11 as System;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
compile_error!("kw-input-helper injects input on Windows, Linux (X11) and macOS only");

const USAGE: &str = "usage: kw-input-helper [--dry-run]";
const IMPLEMENTATION: &str = concat!("kw-input-helper ", env!("CARGO_PKG_VERSION"));
const DRY_IMPLEMENTATION: &str = concat!("kw-input-helper ", env!("CARGO_PKG_VERSION"), " dry-run");
/// While a grant is live the clock is looked at least this often, whatever it does meanwhile.
const LONGEST_WAIT: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    let Some(dry) = dry_run(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let (events, inbox) = mpsc::channel();
    #[cfg(unix)]
    input_helper::posix::forward_stops(events.clone());
    #[cfg(windows)]
    input_helper::win32::forward_stops(events.clone());
    let code = if dry {
        serve(Session::new(Dry, DRY_IMPLEMENTATION), events, &inbox)
    } else {
        match System::open(events.clone()) {
            Ok(system) => serve(Session::new(system, IMPLEMENTATION), events, &inbox),
            Err(problem) => {
                eprintln!("kw-input-helper: {problem}");
                1
            }
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn dry_run(mut arguments: impl Iterator<Item = String>) -> Option<bool> {
    let dry = match arguments.next() {
        None => false,
        Some(argument) if argument == "--dry-run" => true,
        Some(_) => return None,
    };
    arguments.next().is_none().then_some(dry)
}

/// The helper's one loop: a line from the console, physical input, a stop, or the grant's end,
/// each answered in turn. Returns the exit code.
fn serve<B: Backend>(mut session: Session<B>, events: Sender<Event>, inbox: &Receiver<Event>) -> i32 {
    thread::spawn(move || read_console(io::stdin().lock(), &events));
    let mut console = io::stdout().lock();
    loop {
        let mut replies = Vec::new();
        let mut flow = session.tick(wall_ms(), &mut replies);
        if flow == Flow::Go {
            let event = match session.deadline() {
                None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(deadline) => inbox.recv_timeout(wait_until(deadline, wall_ms())),
            };
            flow = match event {
                Ok(Event::Line(line)) => session.receive(line.as_deref(), wall_ms(), &mut replies),
                Ok(Event::Physical) => {
                    session.local_input(wall_ms(), &mut replies);
                    Flow::Go
                }
                Ok(Event::Closed | Event::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    session.closed(&mut replies)
                }
                Err(RecvTimeoutError::Timeout) => Flow::Go,
            };
        }
        let heard = write(&mut console, &replies).is_ok();
        if !heard {
            // Nobody is listening any more: let go of everything and leave.
            let _ = session.closed(&mut Vec::new());
        }
        match flow {
            Flow::Exit(code) => return code,
            Flow::Go if !heard => return 0,
            Flow::Go => {}
        }
    }
}

fn read_console(input: impl BufRead, events: &Sender<Event>) {
    for line in Lines::new(input, protocol::MAX_LINE_BYTES) {
        if events.send(Event::Line(line)).is_err() {
            return;
        }
    }
    let _ = events.send(Event::Closed);
}

fn write(console: &mut impl Write, replies: &[Reply]) -> io::Result<()> {
    if replies.is_empty() {
        return Ok(());
    }
    for reply in replies {
        console.write_all(reply.line().as_bytes())?;
    }
    console.flush()
}

fn wall_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64() * 1000.0)
}

fn wait_until(deadline: f64, now: f64) -> Duration {
    Duration::from_secs_f64(((deadline - now) / 1000.0).clamp(0.0, LONGEST_WAIT.as_secs_f64()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = include_str!("../Cargo.toml");

    fn arguments(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|argument| (*argument).to_owned()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn injects_unless_told_to_run_dry_and_refuses_anything_else() {
        assert_eq!(dry_run(arguments(&[])), Some(false));
        assert_eq!(dry_run(arguments(&["--dry-run"])), Some(true));
        assert_eq!(dry_run(arguments(&["--dry"])), None);
        assert_eq!(dry_run(arguments(&["--dry-run", "extra"])), None);
    }

    #[test]
    fn depends_on_windows_sys_and_x11rb_alone_at_exact_versions() {
        let dependencies: Vec<&str> = MANIFEST.lines().filter(|line| line.contains("dependencies")).collect();
        assert_eq!(
            dependencies,
            [
                "[target.'cfg(windows)'.dependencies.windows-sys]",
                "[target.'cfg(any(target_os = \"linux\", check_foreign))'.dependencies.x11rb]"
            ]
        );
        assert!(MANIFEST.contains("version = \"=0.61.2\"") && MANIFEST.contains("version = \"=0.14.0\""));
        assert!(MANIFEST.contains("default-features = false"));
    }

    #[test]
    fn links_no_file_network_pipe_or_registry_api_on_windows() {
        let forbidden = [
            "Win32_Storage",
            "Win32_Networking",
            "Win32_System_IO",
            "Win32_System_Pipes",
            "Win32_System_Registry",
        ];
        for feature in forbidden {
            assert!(!MANIFEST.contains(feature), "{feature} is enabled");
        }
    }
}
