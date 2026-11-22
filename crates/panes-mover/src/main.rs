mod easing;
mod lines;
mod mover;
mod protocol;
mod screen;
mod stats;
mod tracks;
mod win32;

use std::io::{self, BufRead};
use std::process::ExitCode;
use std::thread;

use lines::Lines;
use mover::{Issue, Mover};
use protocol::Event;
use win32::Parent;

const USAGE: &str = "usage: kw-panes-mover [--issue=async|defer]";
const BETTER_ISSUE: Issue = Issue::Defer;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("kw-panes-mover: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), &'static str> {
    let issue = chosen_issue(std::env::args().skip(1)).ok_or(USAGE)?;
    if !win32::enable_per_monitor_dpi() {
        return Err("per-monitor DPI awareness (v2) is unavailable");
    }
    let parent = Parent::discover().ok_or("cannot identify the parent process")?;
    mover::emit(&Event::Ready { parent_pid: parent.pid });
    let mover = Mover::start(parent.pid, issue);
    thread::spawn(move || exit_with(&parent));
    serve(io::stdin().lock(), &mover);
    Ok(())
}

fn chosen_issue(mut arguments: impl Iterator<Item = String>) -> Option<Issue> {
    let issue = match arguments.next() {
        Some(argument) => Issue::named(argument.strip_prefix("--issue=")?)?,
        None => BETTER_ISSUE,
    };
    arguments.next().is_none().then_some(issue)
}

fn exit_with(parent: &Parent) {
    parent.wait_until_gone();
    std::process::exit(0);
}

fn serve(input: impl BufRead, mover: &Mover) {
    for line in Lines::new(input, protocol::MAX_LINE_BYTES) {
        match line.and_then(|text| protocol::parse(&text)) {
            Ok(Some(command)) => mover.obey(command),
            Ok(None) => {}
            Err(reason) => mover.refuse(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = include_str!("../Cargo.toml");

    fn arguments(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter().map(|argument| (*argument).to_owned()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn issues_moves_as_one_batch_unless_told_otherwise() {
        assert_eq!(chosen_issue(arguments(&[])), Some(Issue::Defer));
        assert_eq!(chosen_issue(arguments(&["--issue=defer"])), Some(Issue::Defer));
        assert_eq!(chosen_issue(arguments(&["--issue=async"])), Some(Issue::Async));
    }

    #[test]
    fn refuses_arguments_it_does_not_know() {
        assert_eq!(chosen_issue(arguments(&["--issue=sync"])), None);
        assert_eq!(chosen_issue(arguments(&["--parent=4242"])), None);
        assert_eq!(chosen_issue(arguments(&["--issue=async", "extra"])), None);
    }

    #[test]
    fn depends_on_windows_sys_alone_at_an_exact_version() {
        let dependencies: Vec<&str> = MANIFEST.lines().filter(|line| line.contains("dependencies")).collect();
        assert_eq!(dependencies, ["[dependencies.windows-sys]", "[dev-dependencies.windows-sys]"]);
        assert_eq!(MANIFEST.matches("version = \"=0.61.2\"").count(), 2);
    }

    #[test]
    fn links_no_file_network_pipe_or_registry_api() {
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
