//! `cmux` CLI entry point. A thin shell over [`cmux_cli`]: parse the
//! invocation, handle the no-socket meta-outcomes (`--version`/`--help`), and
//! surface errors with the correct exit code. Command dispatch over the control
//! socket is added next (see `cmux_cli` module docs).

use std::process::ExitCode;

use cmux_cli::{parse_global_options, CliError, ParseOutcome};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {}", error.message);
            ExitCode::from(error.exit_code.clamp(0, 255) as u8)
        }
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match parse_global_options(args)? {
        ParseOutcome::PrintVersion => {
            println!("cmux {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ParseOutcome::PrintHelp => {
            println!("{}", usage());
            Ok(())
        }
        ParseOutcome::Command { command, .. } => {
            // Command classification (no-socket taxonomy) and the named-pipe
            // transport dispatch land in the next slice; until then an
            // identified command is reported as unrecognized.
            Err(CliError::new(format!(
                "Unknown command '{command}'. Run 'cmux help' to see available commands."
            )))
        }
    }
}

fn usage() -> &'static str {
    "Usage: cmux <path>|<command> [options]"
}
