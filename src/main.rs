use clap::{Parser, Subcommand};
use std::io::{self, Write};
use std::process::ExitCode;
use torrent_lite::magnet::Magnet;

#[derive(Parser)]
#[command(
    version,
    about = "Offline torrent input inspection (development preview)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect a v1 magnet locally. Does not connect to peers or download data.
    InspectMagnet {
        magnet: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let Cli { command } = Cli::parse();
    let Commands::InspectMagnet { magnet, json } = command;
    let value = match Magnet::parse(&magnet) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let output = if json {
        // The schema contains only strings and cannot fail JSON serialization.
        serde_json::to_string(&value).expect("string-only JSON schema")
    } else {
        let mut text = format!("Info hash: {}", value.info_hash());
        if let Some(name) = &value.display_name {
            text.push_str(&format!("\nName: {}", name.escape_default()));
        }
        text
    };
    match writeln!(io::stdout().lock(), "{output}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("error: unable to write output");
            ExitCode::FAILURE
        }
    }
}
