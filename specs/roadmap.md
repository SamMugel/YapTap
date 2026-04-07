# Roadmap

## Phase 1 — CLI Transcription (current)

**Goal:** `$ yaptap` records from the microphone until the user presses Enter, then prints the Whisper transcript to stdout.

Detailed specs: [cli.md](cli.md), [audio-recording.md](audio-recording.md), [transcription.md](transcription.md), [architecture.md](architecture.md)

### Deliverables
- Rust binary `yaptap` installable via `cargo install` or a pre-built release.
- Starts microphone capture on launch.
- Pressing Enter stops capture and flushes the audio buffer to a temp WAV file.
- Invokes a Python helper (`transcribe.py`) as a subprocess, passing the WAV path.
- `transcribe.py` calls the Whisper Python library and returns the transcript on stdout.
- `yaptap` prints the transcript to the terminal and exits 0.

### Out of scope for phase 1
- Prompt selection, ollama, hotkeys, cursor injection.

---

## Phase 2 — Prompt + LLM Output

**Goal:** `$ yaptap --prompt <name>` records, transcribes, then pipes the transcript through an LLM with a user-selected prompt, streaming the result to stdout.

Detailed specs: [prompts.md](prompts.md), [llm.md](llm.md)

### Deliverables
- CLI flags `--prompt <name>`, `--prompt-file <path>`, `--llm-model <name>`, `--model <name>`, `--list-prompts`.
- Bundled default prompts in `config/prompts/` (TOML format).
- `yaptap --list-prompts` prints available prompts from `config/prompts/` and exits.
- After transcription, Rust spawns `python3 src/core/llm.py --prompt-file <path>` with transcript piped to stdin.
- `llm.py` calls ollama, streams tokens to stdout; Rust echoes them in real time.
- Status line `Thinking...` printed between `Transcribing...` and the first LLM token.
- Without `--prompt` / `--prompt-file`, `yaptap` behaves as in phase 1 (print raw transcript).

### Out of scope for phase 2
- Global hotkeys, background daemon, cursor injection.

---

## Phase 3 — Hotkey + Prompt Menu (high-level)

- A background daemon (`yaptapd`) registers a global hotkey via macOS `CGEventTap`.
- First hotkey press → start recording (visual indicator in menu bar or tray).
- Second hotkey press → stop recording, run transcription + LLM pipeline.
- A lightweight TUI or native menu lists available prompts; user selects before or after invoking the hotkey.

---

## Phase 4 — Cursor Injection (high-level)

- After the LLM response is ready, the output is pasted at the user's current cursor position via macOS `CGEventCreateKeyboardEvent` / `NSPasteboard`.
- The previous clipboard contents are restored after injection.
- Injection is implemented in Rust using macOS accessibility APIs; no Python involved.
