#[cfg(windows)]
mod demo;

fn main() {
    let started = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(windows)]
        Some("demo") => std::process::exit(demo::run(started, &args[1..])),
        _ => {
            println!("usage: vignette demo [panes] [mood] [--hold <ms>] [--shot <path-prefix>]");
            std::process::exit(2);
        }
    }
}
