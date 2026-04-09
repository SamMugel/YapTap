# Global Hotkey (Phase 3)

## Overview

The global hotkey starts and stops recording from anywhere on the system without the user switching to `yaptap`. The default hotkey is **Option+Space** (⌥Space). It is configurable via `~/.config/yaptap/config.toml`.

---

## Accessibility Permission

Registering a `CGEventTap` at the HID level requires macOS Accessibility permission.

On launch, if permission is absent:

1. Show an alert:
   > *"YapTap needs Accessibility access to capture the global hotkey.*
   > *Open System Settings → Privacy & Security → Accessibility?"*
2. Buttons: **Open Settings** / **Later**.
3. **Open Settings** calls `open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"`.
4. The app continues running with the menu bar icon visible; hotkey remains inactive.
5. Re-launch the app after granting permission to activate the hotkey.

---

## Hotkey State Machine

```
IDLE
 │
 │  [hotkey press #1]
 ▼
RECORDING
 │
 │  [hotkey press #2]
 ▼
PROCESSING
 │
 │  [pipeline complete]
 ▼
IDLE
```

Additional rules:
- A hotkey press in PROCESSING state is ignored (pipeline already running).
- **Quit YapTap** from any state → stop capture, kill subprocesses, delete temp files, exit 0.

---

## Step-by-step Flow

### Hotkey press #1 — start recording

1. App transitions `IDLE → RECORDING`.
2. Menu bar icon switches to Active variant.
3. Audio capture starts (`cpal` pipeline; same format as CLI mode).

### Hotkey press #2 — stop recording and run pipeline

1. Audio capture stops; PCM buffer is flushed to a temp WAV file.
2. App transitions `RECORDING → PROCESSING` (icon stays Active).
3. Transcription: `transcribe.py` subprocess is spawned with the WAV path.
4. If the selected prompt is not **No Prompt**: `llm.py` subprocess is spawned with the transcript on stdin and the prompt file path as `--prompt-file`. Rust reads the subprocess stdout into a buffer until EOF; the complete text is assembled in memory before the next step (unlike CLI mode, tokens are not echoed to a terminal as they arrive).
5. If **No Prompt** is selected: the raw transcript is used directly.
6. The complete text (transcript or LLM output) is written to the system clipboard via `NSPasteboard` as `NSPasteboardTypeString`.
7. Temp WAV file is deleted.
8. App transitions `PROCESSING → IDLE`; icon returns to Idle variant.

---

## Clipboard Output

- The full text is placed on the general pasteboard, replacing the previous contents.
- Previous clipboard contents are **not** restored (unlike Phase 4 cursor injection).
- There is no terminal output in app mode; the clipboard is the sole output channel.

---

## Hotkey Registration

- Implemented via macOS `CGEventTap` at `kCGHIDEventTap` level.
- Listens for `kCGEventKeyDown` events matching the configured key + modifier mask.
- The event tap is passive (it does not consume or suppress the hotkey event by default). If the hotkey conflicts with another app, the event still reaches both.

---

## Hotkey Conflict

If `CGEventTap` fails to register (e.g. permission denied or system-level conflict):

1. Log the error to stderr.
2. Show a one-time alert: *"The hotkey '<configured-hotkey>' could not be registered. Edit ~/.config/yaptap/config.toml to choose a different hotkey, then restart YapTap."* (where `<configured-hotkey>` is the value read from the config file, e.g. `option+space`)
3. The app remains running; the menu bar icon is still functional.

---

## Configuration

The hotkey is read once at launch from `~/.config/yaptap/config.toml`:

```toml
hotkey = "option+space"
```

See [config.md](config.md) for the full key-name syntax and how to change the hotkey.
