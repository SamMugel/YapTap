# Architecture

## Platform

**macOS only.** Do not add cross-platform abstractions or stubs for Linux/Windows unless explicitly requested.

---

## Language Boundary Rule

| Concern | Language | Reason |
|---|---|---|
| Audio capture | Rust | OS-level audio API (`cpal`); requires low-latency, no GIL |
| Process orchestration / CLI | Rust | Binary entrypoint, signal handling, subprocess management |
| Transcription | Python (subprocess) | Whisper is a Python library; called via `transcribe.py` |
| LLM inference (phase 2+) | Python (subprocess) | Ollama Python client; called via `llm.py` |
| Global hotkey / event tap (phase 3+) | Rust | macOS `CGEventTap` — OS-critical |
| Clipboard / cursor injection (phase 4) | Rust | macOS `NSPasteboard` / `CGEvent` — OS-critical |

**Rule:** anything that directly touches OS APIs (input events, audio hardware, accessibility, clipboard) must be Rust. Python is only ever invoked as a short-lived subprocess for ML inference tasks.

---

## Phase 1 Component Diagram

```
┌─────────────────────────────────────┐
│           yaptap (Rust binary)      │
│                                     │
│  ┌─────────────┐                    │
│  │  cpal audio │  PCM samples       │
│  │  capture    │──────────────────► │
│  └─────────────┘   ring buffer      │
│                                     │
│  [user presses Enter]               │
│         │                           │
│         ▼                           │
│  encode buffer → temp .wav file     │
│         │                           │
│         ▼                           │
│  spawn subprocess:                  │
│    python3 src/core/transcribe.py   │
│             <wav_path>              │
│         │                           │
│         ▼  (stdout)                 │
│  print transcript to terminal       │
└─────────────────────────────────────┘
```

---

## Directory Layout (phase 1)

```
YapTap/
├── Cargo.toml
├── PRD.md
├── src/
│   ├── main.rs                  # Rust CLI entrypoint, audio capture, subprocess call
│   └── core/
│       ├── transcribe.py        # Whisper wrapper: WAV path → transcript on stdout
│       └── transcribe_test.py   # co-located unit tests for transcribe.py
├── specs/               # this directory
└── SPECS.md
```

---

## Subprocess Contract

`yaptap` communicates with Python scripts through a simple stdio contract:

- **stdin:** nothing (the WAV path is passed as a CLI argument)
- **stdout:** UTF-8 text, newline-terminated transcript (or JSON in later phases)
- **stderr:** forwarded directly to the terminal for debugging
- **exit code:** 0 on success, non-zero on any error; `yaptap` propagates the error to the user

---

## Dependencies (phase 1)

### Rust
- [`cpal`](https://crates.io/crates/cpal) — cross-platform audio capture
- [`hound`](https://crates.io/crates/hound) — WAV encoding
- [`tempfile`](https://crates.io/crates/tempfile) — temporary WAV storage
- [`anyhow`](https://crates.io/crates/anyhow) — error handling in the binary (`anyhow::Result<T>` + `.context()`)
- [`ctrlc`](https://crates.io/crates/ctrlc) — SIGINT handler for temp file cleanup on Ctrl-C
- [`tracing`](https://crates.io/crates/tracing) — structured internal logging (debug/warn/error spans)
- [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) — subscriber wiring in `main` (controlled via `RUST_LOG`)

Note: user-facing terminal output (the transcript, status lines) uses `println!`/`eprintln!` — that is intentional UI output. `tracing` is for internal observability only.

### Python
- `openai-whisper` — transcription (installed in user environment)
- Standard library only (`argparse`, `sys`, `pathlib`, `logging`)
