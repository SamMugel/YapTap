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
- CLI flags `--prompt <name>`, `--prompt-file <path>`, `--llm-model <name>`, `--llm-provider <name>`, `--model <name>`, `--list-prompts`.
- Bundled default prompts in `config/prompts/` (TOML format).
- `yaptap --list-prompts` prints available prompts from `config/prompts/` and exits.
- After transcription, Rust spawns `python3 src/core/llm.py --prompt-file <path> [--model <name>] [--provider <name>]` with transcript piped to stdin.
- `llm.py` calls the configured LLM provider (ollama or CompactifAI), streams tokens to stdout; Rust echoes them in real time.
- `llm_provider` config field selects the provider (`"ollama"` default, `"compactifai"` for cloud); `--llm-provider` CLI flag overrides it per-invocation.
- Status line `Thinking...` printed between `Transcribing...` and the first LLM token.
- Without `--prompt` / `--prompt-file`, `yaptap` behaves as in phase 1 (print raw transcript).

### Out of scope for phase 2
- Global hotkeys, background daemon, cursor injection.

---

## Phase 3 — Menu Bar App + Global Hotkey

**Goal:** `yaptap` with no flags starts a persistent macOS menu bar app. A global hotkey (⌥Space by default) starts and stops recording; the result is placed on the clipboard. The user selects an active prompt from the menu bar dropdown.

Detailed specs: [menubar.md](menubar.md), [hotkey.md](hotkey.md), [config.md](config.md), [packaging.md](packaging.md)

### Deliverables
- `yaptap` with no args starts as a menu bar app (`NSApplication` in `LSUIElement` mode; no Dock icon).
- Menu bar icon: three-bar equalizer, two variants — Idle (bars at 6:10:6 heights) and Active (bars at 10:10:10).
- Dropdown menu listing all prompts from `config/prompts/`; sticky prompt selection.
- Global hotkey (default ⌥Space): first press starts recording, second press stops and runs the pipeline.
- LLM output (or raw transcript if No Prompt) placed on the system clipboard via `NSPasteboard`.
- Config file `~/.config/yaptap/config.toml` persists hotkey and prompt selection across launches.
- Single-instance guard via lock file (`~/.config/yaptap/yaptap.lock`).
- Accessibility permission prompt on first launch if not already granted.
- CLI modes (`--prompt`, `--list-prompts`, `--device`, etc.) continue to work exactly as in Phase 2.
- In-app hotkey configuration: clicking the Hotkey menu item opens an input dialog; the new hotkey takes effect immediately without restarting the app and is persisted to the config file.
- `Makefile` with `build`, `icns`, `app`, `dmg`, and `clean` targets.
- `YapTap.app` bundle: Rust binary, bundled prompt TOML files, menu bar PNG icons, Python scripts, and `Info.plist`.
- `YapTap.dmg` disk image with drag-to-Applications install UX (built by `make dmg` via `hdiutil`).
- App icon (`YapTap.icns`) generated from the menu bar idle PNG using `sips` + `iconutil`.
- First-launch Python setup: venv created at `~/.config/yaptap/.venv/`; `openai-whisper` and `ollama` installed silently; user alerted if `ffmpeg` is missing.
- Whisper model downloaded automatically on first recording attempt (cached in `~/.cache/whisper/`).
- Ollama availability check before LLM step; graceful alert if Ollama is not running.
- Resource path resolution: binary locates scripts, prompts, and icons relative to `Contents/Resources/` when running inside the bundle; falls back to project root during development.

### Out of scope for Phase 3
- Cursor injection at current caret position (Phase 4).
- System notifications.

---

## Phase 4 — Cursor Injection (high-level)

- After the LLM response is ready, the output is pasted at the user's current cursor position via macOS `CGEventCreateKeyboardEvent` / `NSPasteboard`.
- The previous clipboard contents are restored after injection.
- Injection is implemented in Rust using macOS accessibility APIs; no Python involved.
