use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use torrent_lite::magnet::Magnet;
use torrent_lite::torrent_file::inspect_torrent;

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
    /// Inspect a local v1 .torrent file. Does not download data.
    InspectTorrent {
        path: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let Cli { command } = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.exit()
        }
        Err(_) => {
            // clap diagnostics can include arbitrary arguments, including tracker secrets.
            eprintln!("error: invalid command arguments; run torrent-lite --help");
            return ExitCode::from(2);
        }
    };
    let output = match command {
        Commands::InspectMagnet { magnet, json } => {
            let value = match Magnet::parse(&magnet) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(2);
                }
            };
            if json {
                serde_json::to_string(&value).expect("string-only JSON schema")
            } else {
                let mut text = format!("Info hash: {}", value.info_hash());
                if let Some(name) = &value.display_name {
                    text.push_str(&format!("\nName: {}", name.escape_default()));
                }
                text
            }
        }
        Commands::InspectTorrent { path, json } => {
            let file = match File::open(path) {
                Ok(file) => file,
                Err(_) => {
                    eprintln!("error: unable to open torrent file");
                    return ExitCode::from(2);
                }
            };
            let mut bytes = Vec::new();
            if file
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .is_err()
            {
                eprintln!("error: unable to read torrent file");
                return ExitCode::from(2);
            }
            let value = match inspect_torrent(&bytes) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(2);
                }
            };
            if json {
                serde_json::to_string(&value).expect("summary has only plain values")
            } else {
                format!(
                    "Info hash: {}\nName: {}\nFiles: {}\nBytes: {}",
                    value.info_hash,
                    value.name.escape_default(),
                    value.files.len(),
                    value.total_bytes
                )
            }
        }
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
