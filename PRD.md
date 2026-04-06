# YapTap — Product Requirements Document

This is the authoritative feature reference for YapTap. Per project rules, no feature may be added to code unless it is described here.

---

## Platform

**macOS only.** No Linux or Windows support.

---

## Phase 1 — CLI Transcription (in scope now)

| # | Requirement |
|---|---|
| P1-1 | `$ yaptap` starts recording from the default microphone immediately on launch |
| P1-2 | A status line is printed: `Recording... (press Enter to stop)` with an elapsed timer |
| P1-3 | Pressing Enter stops recording |
| P1-4 | The captured audio is transcribed using Whisper (`base` model by default) |
| P1-5 | The transcript is printed to stdout, followed by a newline |
| P1-6 | The process exits 0 on success |
| P1-7 | On SIGINT (Ctrl-C), recording stops, the temp file is deleted, and the process exits 130 |
| P1-8 | Meaningful error messages are printed to stderr and the process exits 1 on device or transcription failure |

## Phase 2 — Prompt + LLM Output (future)

| # | Requirement |
|---|---|
| P2-1 | `--prompt <name>` selects a named prompt from `~/.config/yaptap/prompts/` |
| P2-2 | `--prompt-file <path>` selects a custom prompt file |
| P2-3 | Transcript + prompt are sent to a local Ollama model |
| P2-4 | LLM response is streamed and printed to stdout |

## Phase 3 — Hotkey + Prompt Menu (future)

| # | Requirement |
|---|---|
| P3-1 | A background daemon registers a global hotkey via macOS `CGEventTap` |
| P3-2 | First hotkey press starts recording; second stops it and runs the pipeline |
| P3-3 | A menu (TUI or native) lets the user select a prompt |

## Phase 4 — Cursor Injection (future)

| # | Requirement |
|---|---|
| P4-1 | LLM output is pasted at the active cursor position via macOS `NSPasteboard` / `CGEvent` |
| P4-2 | The user's clipboard contents are restored after injection |
