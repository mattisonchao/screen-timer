# Screen Timer

A macOS menu bar timer to enforce context switching. Set a focus duration, and when time's up, your screen gets taken over with a full-screen break overlay across all displays.

## Features

- **Menu bar tray icon** with quick-start presets (25m, 45m, 1h, 1h30m, 2h)
- **Pomodoro mode** — auto-cycle focus and break sessions (e.g. 4x25m focus + 5m short breaks + 15m long break)
- **Full-screen break overlay** across all connected monitors
- **CLI control** — start, stop, pause, resume, and query status from the terminal or scripts
- **IPC** over Unix socket for integration with automation tools and AI agents
- **Persistent config** — remembers your last settings across restarts
- **Custom break images** — drop images into `~/.config/screen-timer/images/`
- **Emergency unlock** — press Escape 3 times to exit the break screen early

## Install

### Homebrew

```sh
brew tap mattisonchao/tap
brew install screen-timer
```

### From source

```sh
git clone https://github.com/mattisonchao/screen-timer.git
cd screen-timer
cargo build --release
cp target/release/screen-timer /usr/local/bin/
```

### GitHub Releases

Download the universal macOS binary from [Releases](https://github.com/mattisonchao/screen-timer/releases).

## Usage

### GUI

Run without arguments to start the menu bar app:

```sh
screen-timer
```

### CLI

Control a running instance from the terminal:

```sh
screen-timer start 25m       # Start a 25-minute timer
screen-timer start 1h30m     # Start a 1.5-hour timer
screen-timer pomodoro         # Start a Pomodoro cycle
screen-timer pause            # Pause the timer
screen-timer resume           # Resume the timer
screen-timer stop             # Stop the timer
screen-timer status           # Show status (JSON)
screen-timer unlock           # Emergency unlock break screen
screen-timer quit             # Quit the app
```

Duration formats: `1h`, `30m`, `1h30m`, `90s`, or bare number for minutes (`25` = 25 minutes).

### Pomodoro Mode

Toggle Pomodoro mode in the app's setup screen. Default cycle:

- 4 x 25-minute focus sessions
- 5-minute short breaks between sessions
- 15-minute long break after completing all sessions (full-screen)

All values are configurable in the UI or via `~/.config/screen-timer/config.json`.

### Configuration

Settings are persisted to `~/.config/screen-timer/config.json`:

```json
{
  "last_hours": 0,
  "last_minutes": 25,
  "last_seconds": 0,
  "break_length_secs": 30,
  "pomodoro_enabled": false,
  "pomodoro": {
    "focus_minutes": 25,
    "short_break_minutes": 5,
    "long_break_minutes": 15,
    "sessions_before_long_break": 4
  }
}
```

### Break Images

Add `.png`, `.jpg`, `.jpeg`, or `.webp` images to `~/.config/screen-timer/images/`. A random image will be shown as the break screen background.

## License

MIT
