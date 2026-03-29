# Debug Logging & Crash Capture

NextAI Translator includes a built-in logging system to help diagnose background crashes on Windows.

## Log File Locations

### Windows

| File | Path | Purpose |
|------|------|---------|
| `app.log` | `%APPDATA%\xyz.yetone.apps.openai-translator\logs\app.log` | General application log (lifecycle events, warnings, errors) |
| `crash.log` | `%APPDATA%\xyz.yetone.apps.openai-translator\logs\crash.log` | Panic crash reports with full backtrace |

Rotated log files (e.g., `app.log.1`, `app.log.2`) are kept in the same directory when `app.log` exceeds 5 MB.

### macOS

| File | Path |
|------|------|
| `app.log` | `~/Library/Logs/xyz.yetone.apps.openai-translator/app.log` |
| `crash.log` | `~/Library/Logs/xyz.yetone.apps.openai-translator/crash.log` |

### Linux

| File | Path |
|------|------|
| `app.log` | `~/.config/xyz.yetone.apps.openai-translator/logs/app.log` |
| `crash.log` | `~/.config/xyz.yetone.apps.openai-translator/logs/crash.log` |

## Quick Access via System Tray

Right-click the tray icon and select **"View Logs"** to open the log directory in your file manager.

## After a Crash

1. Right-click tray icon > **View Logs** (or navigate to the path above manually)
2. Check **`crash.log`** first — if it has a recent entry, a Rust panic caused the crash. The backtrace shows the exact function and line number.
3. Check **`app.log`** — scroll to the bottom. The last lines before the log stops show what the app was doing before it died.
4. If `crash.log` has no new entries and `app.log` just stops abruptly, the process was likely killed externally (Windows Update, antivirus, OOM).

## What Gets Logged

- **App lifecycle**: startup, setup completion, ready state, exit
- **Mouse hook**: config load failures, mouse position errors
- **Update checker**: periodic check results, failures
- **IPC server**: binding status, request errors
- **OCR**: Windows OCR API errors
- **Fetch**: streaming request info, abort events
- **Panics**: thread name, location, message, full backtrace (written to `crash.log`)
