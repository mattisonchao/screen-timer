use clap::{Parser, Subcommand};

mod app;
mod config;
mod ipc;

/// Screen Timer — menu bar timer to enforce context switching.
///
/// Run without arguments to start the menu bar app.
/// Use subcommands to control a running instance (e.g. from scripts or agents).
#[derive(Parser)]
#[command(name = "screen-timer", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a timer (e.g. `screen-timer start 1h`, `screen-timer start 25m`)
    Start {
        /// Duration: 1h, 30m, 1h30m, or bare number (minutes)
        duration: String,
    },
    /// Stop the current timer
    Stop,
    /// Pause the current timer
    Pause,
    /// Resume a paused timer
    Resume,
    /// Show timer status (JSON output for scripts)
    Status,
    /// Emergency unlock the break screen
    Unlock,
    /// Quit the running daemon
    Quit,
    /// Start a Pomodoro cycle with saved settings
    Pomodoro,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Daemon mode: start the menu bar app
            if let Err(e) = app::run() {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(cmd) => {
            let ipc_cmd = match cmd {
                Commands::Start { duration } => {
                    let secs = match parse_duration(&duration) {
                        Some(s) if s >= 10 => s,
                        Some(_) => {
                            eprintln!("Error: Minimum duration is 10 seconds");
                            std::process::exit(1);
                        }
                        None => {
                            eprintln!("Error: Invalid duration '{duration}'");
                            eprintln!("Examples: 1h, 30m, 1h30m, 25 (bare number = minutes)");
                            std::process::exit(1);
                        }
                    };
                    ipc::IpcCommand::Start { seconds: secs }
                }
                Commands::Stop => ipc::IpcCommand::Stop,
                Commands::Pause => ipc::IpcCommand::Pause,
                Commands::Resume => ipc::IpcCommand::Resume,
                Commands::Status => ipc::IpcCommand::Status,
                Commands::Unlock => ipc::IpcCommand::Unlock,
                Commands::Quit => ipc::IpcCommand::Quit,
                Commands::Pomodoro => ipc::IpcCommand::StartPomodoro,
            };

            match ipc::send_command(&ipc_cmd) {
                Ok(resp) => match resp {
                    ipc::IpcResponse::Ok { message } => println!("{message}"),
                    ipc::IpcResponse::Status {
                        running,
                        paused,
                        remaining,
                        total,
                    } => {
                        let status = serde_json::json!({
                            "running": running,
                            "paused": paused,
                            "remaining_seconds": remaining,
                            "total_seconds": total,
                            "remaining_display": format_time(remaining),
                            "total_display": format_time(total),
                        });
                        println!("{}", serde_json::to_string_pretty(&status).unwrap());
                    }
                    ipc::IpcResponse::Error(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn parse_duration(input: &str) -> Option<u64> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(n) = input.parse::<u64>() {
        return Some(n * 60);
    }
    let mut total: u64 = 0;
    let mut num_buf = String::new();
    let mut found_unit = false;
    for c in input.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let n: u64 = num_buf.parse().ok()?;
            num_buf.clear();
            match c {
                'h' | 'H' => total += n * 3600,
                'm' | 'M' => total += n * 60,
                's' | 'S' => total += n,
                _ => return None,
            }
            found_unit = true;
        }
    }
    if !num_buf.is_empty() {
        if found_unit {
            total += num_buf.parse::<u64>().ok()?;
        } else {
            return None;
        }
    }
    if total == 0 { None } else { Some(total) }
}

fn format_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
