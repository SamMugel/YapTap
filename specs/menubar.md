# Menu Bar App (Phase 3)

## Overview

In Phase 3, `yaptap` with no flags launches as a long-running macOS menu bar app. The app lives in the menu bar, registers a global hotkey, and places LLM output directly on the clipboard.

The CLI modes from Phases 1 and 2 still work when any flag is passed (see [cli.md](cli.md)).

---

## App Mode vs CLI Mode

| Invocation | Mode |
|---|---|
| `yaptap` (no args) | Menu bar app |
| `yaptap --prompt <name>` | CLI mode (Phase 2) |
| `yaptap --prompt-file <path>` | CLI mode (Phase 2) |
| `yaptap --list-prompts` | CLI utility (Phase 2) |
| `yaptap --model <name>` | CLI mode (Phase 2) |
| `yaptap --llm-model <name>` | CLI mode (Phase 2) |
| `yaptap --device <index>` | CLI mode (Phase 3) |

---

## Lifecycle

1. Process starts; reads `~/.config/yaptap/config.toml` (creates with defaults if absent).
2. Loads available prompts from `<resources_dir>/config/prompts/` (see [packaging.md](packaging.md) for path resolution).
3. Creates an `NSApplication` in `LSUIElement` mode (no Dock icon, no app menu bar).
4. Adds the menu bar item with the idle icon.
5. Registers the configured global hotkey via `CGEventTap`.
6. If Accessibility permission is not granted, shows an alert (see [hotkey.md](hotkey.md)).
7. Runs the `NSRunLoop` indefinitely until the user selects **Quit YapTap**.

**Model selection in app mode:** the Whisper model and Ollama model are read from `whisper_model` and `llm_model` in the config file (see [config.md](config.md)). There is no in-app UI for model selection; edit the config file directly.

**Single-instance guard:** on launch the app writes its PID to `~/.config/yaptap/yaptap.lock`. If that file already exists, the new process reads the stored PID and checks whether it is still alive (`kill -0 <pid>`). If alive, the new process exits 0 immediately. If the process is dead (stale lock from a crash), the new process overwrites the lock file with its own PID and continues normally. The lock file is deleted on clean exit (including **Quit YapTap** and SIGTERM).

---

## Icon

### Design

Three vertical filled rectangles, horizontally centred, arranged as a minimal audio equalizer. Rendered as a macOS template image (monochrome; the system adapts it for light and dark menu bar themes automatically).

| Property | Value |
|---|---|
| Bar width | 3 pt each |
| Gap between bars | 2 pt |
| Bar heights — idle | left 6 pt, centre 10 pt, right 6 pt |
| Bar heights — active | 10 pt, 10 pt, 10 pt |
| Bounding box | 18 × 18 pt |
| Format | PNG template image; provide @1× and @2× (Retina) |

### States

| App state | Icon variant | Visual |
|---|---|---|
| Idle | Idle | Outer bars shorter than centre (6:10:6) |
| Recording | Active | All three bars equal height (10:10:10) |
| Processing (transcription + LLM) | Active | All three bars equal height (10:10:10) |

On error the icon returns to Idle; an alert dialog surfaces the error message.

---

## Menu Structure

Clicking the menu bar icon opens a dropdown:

```
   Start Recording           ← toggles to Stop Recording while recording;
                               shows Processing… (disabled) while transcribing/LLM
   ─────────────────────────
   Action Items
✓  Email Reply
   Journal
   Meeting Notes
   Slack Message
   ─────────────────────────
   No Prompt
   ─────────────────────────
   Hotkey: ⌥Space            ← click to change
   Open Config…
   ─────────────────────────
   Quit YapTap
```

The **Start / Stop Recording** toggle is the first item in the dropdown:

| App state | Item label | Enabled |
|---|---|---|
| Idle | Start Recording | Yes |
| Recording | Stop Recording | Yes |
| Processing | Processing… | No (disabled) |

Clicking **Start Recording** in Idle state begins a recording (equivalent to pressing the hotkey). Clicking **Stop Recording** in Recording state stops it. The item is disabled and shows **Processing…** while transcription or LLM inference is in progress.

- Prompt entries are loaded from `config/prompts/` at launch and refreshed each time the menu is opened, sorted alphabetically by filename stem (matching `--list-prompts` output order).
- The currently selected prompt has a checkmark (✓). Only one item is checked at a time.
- **No Prompt** puts the raw transcript on the clipboard with no LLM step.
- The hotkey display item shows the currently configured hotkey. Clicking it opens a hotkey-change dialog (see Hotkey Selection below).
- **Open Config…** opens `~/.config/yaptap/config.toml` in the default text editor via `open`. Changes to `whisper_model` or `llm_model` require restarting YapTap to take effect. Changes to `hotkey` made directly in the file require a restart; use the in-app hotkey dialog for immediate effect. Changes to `selected_prompt` made by hand are overwritten the next time the user picks from the menu.
- **Quit YapTap** kills any in-progress subprocess, deletes temp files, removes the lock file, and exits 0.

### Hotkey Selection

Clicking the Hotkey menu item opens an input dialog pre-filled with the current hotkey string (e.g. `option+space`). The user edits the string and clicks OK. The app validates the input with the same parser used at launch (see [config.md](config.md) for the key-name syntax). On valid input:

1. The rdev listener updates its target combo immediately — no restart required.
2. The new value is written atomically to `~/.config/yaptap/config.toml` (see [config.md](config.md)).
3. The menu label updates to reflect the new hotkey (e.g. `Hotkey: ⌘⇧Y`).

On invalid input: an error alert is shown and the old hotkey remains active. Clicking Cancel makes no change.

### Prompt Selection Persistence

Selecting a prompt writes `selected_prompt = "<stem>"` to the config file immediately using an atomic write (see [config.md](config.md)). The checkmark updates in the menu. If a recording is already in progress when the selection changes, the new prompt takes effect for the current recording.

---

## Error Handling

| Condition | Behaviour |
|---|---|
| `config/prompts/` absent or empty at launch | Menu shows a disabled *"No prompts found"* item; only **No Prompt** is active |
| Accessibility permission denied | Alert on launch; hotkey not registered until permission is granted and app is restarted |
| Audio device lost during recording | Transition to Idle; show alert with error message |
| Transcription failure | Transition to Idle; show alert with error message |
| Ollama not running (TCP probe fails) | Alert: *"Ollama is not running. Start Ollama and try again."*; transition to Idle; `llm.py` is not spawned |
| LLM failure | Transition to Idle; show alert with error message |
| Config file write failure | Log warning to stderr; continue with in-memory selection only |
