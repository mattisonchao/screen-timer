use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;

pub const SOCKET_PATH: &str = "/tmp/screen-timer.sock";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum IpcCommand {
    Start { seconds: u64 },
    Stop,
    Pause,
    Resume,
    Status,
    SetImage { path: String },
    Unlock,
    Quit,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum IpcResponse {
    Ok { message: String },
    Status {
        running: bool,
        paused: bool,
        remaining: u64,
        total: u64,
    },
    Error(String),
}

/// Send a command to the running daemon
pub fn send_command(cmd: &IpcCommand) -> Result<IpcResponse, String> {
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|e| format!("Cannot connect to screen-timer daemon: {e}\nIs it running? Start with: screen-timer"))?;

    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();

    let msg = serde_json::to_vec(cmd).map_err(|e| e.to_string())?;
    stream.write_all(&msg).map_err(|e| e.to_string())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| e.to_string())?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

/// Start IPC server in a background thread
pub fn start_server(tx: mpsc::Sender<IpcCommand>, egui_ctx: egui::Context) {
    // Clean up stale socket
    let _ = std::fs::remove_file(SOCKET_PATH);

    std::thread::spawn(move || {
        let listener = match UnixListener::bind(SOCKET_PATH) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind IPC socket: {e}");
                return;
            }
        };

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .ok();

                    let mut buf = Vec::new();
                    if stream.read_to_end(&mut buf).is_err() {
                        continue;
                    }

                    if let Ok(cmd) = serde_json::from_slice::<IpcCommand>(&buf) {
                        let _ = tx.send(cmd);
                        egui_ctx.request_repaint();

                        // Give main thread a moment to process, then we'll
                        // send a generic OK (status queries are handled inline)
                        std::thread::sleep(std::time::Duration::from_millis(50));

                        let resp = IpcResponse::Ok {
                            message: "Command received".to_string(),
                        };
                        let resp_bytes = serde_json::to_vec(&resp).unwrap_or_default();
                        let _ = stream.write_all(&resp_bytes);
                    }
                }
                Err(_) => continue,
            }
        }
    });
}

use eframe::egui;

pub fn cleanup() {
    let _ = std::fs::remove_file(SOCKET_PATH);
}

pub fn images_dir() -> PathBuf {
    let dir = dirs_fallback().join("images");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn config_dir() -> PathBuf {
    let dir = dirs_fallback();
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn dirs_fallback() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("screen-timer")
    } else {
        PathBuf::from("/tmp/screen-timer-config")
    }
}
