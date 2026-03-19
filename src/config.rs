use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::ipc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PomodoroConfig {
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub sessions_before_long_break: u32,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            sessions_before_long_break: 4,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub last_hours: u32,
    pub last_minutes: u32,
    pub last_seconds: u32,
    pub break_length_secs: u64,
    pub pomodoro_enabled: bool,
    pub pomodoro: PomodoroConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_hours: 0,
            last_minutes: 25,
            last_seconds: 0,
            break_length_secs: 30,
            pomodoro_enabled: false,
            pomodoro: PomodoroConfig::default(),
        }
    }
}

impl AppConfig {
    fn config_path() -> PathBuf {
        ipc::config_dir().join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}
