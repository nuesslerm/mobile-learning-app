//! The static `logger` binary the VM's shell scripts and vm-ops-api call: `logger emit ...`, the same line as `wf log emit`.
use telemetry::cli;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.split_first() {
        Some((command, rest)) if command == "emit" => match cli::parse(rest) {
            Ok(emit) => cli::run(emit),
            Err(why) => {
                eprintln!("logger: {why}\n{}", cli::USAGE);
                2
            }
        },
        _ => {
            eprintln!("{}", cli::USAGE);
            2
        }
    };
    std::process::exit(code);
}
