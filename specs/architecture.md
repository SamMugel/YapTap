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

## Directory Layout (phase 2)

```
YapTap/
├── Cargo.toml
├── src/
│   ├── main.rs                  # Rust CLI entrypoint, audio capture, subprocess calls
│   └── core/
│       ├── transcribe.py        # Whisper wrapper: WAV path → transcript on stdout
│       ├── transcribe_test.py   # co-located unit tests for transcribe.py
│       ├── llm.py               # Ollama wrapper: stdin transcript + prompt → streamed response
│       └── llm_test.py          # co-located unit tests for llm.py
├── config/
│   └── prompts/                 # bundled default prompt TOML files (binary-relative at runtime)
│       ├── email-reply.toml
│       ├── meeting-notes.toml
│       ├── slack-message.toml
│       ├── action-items.toml
│       └── journal.toml
├── PRD/
│   ├── PRD_1.json
│   └── PRD_2.json
├── specs/               # this directory
└── SPECS.md
```

At runtime, Rust resolves the prompts directory as `<binary_dir>/config/prompts/` using `std::env::current_exe()`.

---

## Phase 2 Component Diagram

```
┌─────────────────────────────────────────────┐
│             yaptap (Rust binary)            │
│                                             │
│  parse flags (--prompt / --prompt-file)     │
│  resolve prompt TOML path                   │
│         │                                   │
│         ▼                                   │
│  [audio capture → WAV → transcribe.py]      │
│         │ (transcript text)                 │
│         ▼                                   │
│  print "Thinking..."                        │
│         │                                   │
│         ▼                                   │
│  spawn subprocess:                          │
│    python3 src/core/llm.py                  │
│      --prompt-file <path>                   │
│      [--model <name>]                       │
│    stdin ← transcript                       │
│         │ (streamed stdout)                 │
│         ▼                                   │
│  echo tokens to terminal in real time       │
└─────────────────────────────────────────────┘
```

---

## Subprocess Contract

`yaptap` communicates with Python scripts through a simple stdio contract:

### `transcribe.py`
- **stdin:** nothing (WAV path is a CLI argument)
- **stdout:** UTF-8 transcript text, newline-terminated
- **stderr:** forwarded to terminal for debugging
- **exit code:** 0 on success, non-zero on error

### `llm.py`
- **stdin:** transcript text (UTF-8, newline-terminated)
- **stdout:** LLM response, streamed token by token; final `\n` at end
- **stderr:** forwarded to terminal for debugging
- **exit code:** 0 on success, non-zero on error

`yaptap` propagates non-zero exit codes to the user as an error message and exits 1.

---

## Dependencies (phases 1 & 2)

### Rust
- [`cpal`](https://crates.io/crates/cpal) — cross-platform audio capture
- [`hound`](https://crates.io/crates/hound) — WAV encoding
- [`tempfile`](https://crates.io/crates/tempfile) — temporary WAV storage
- [`anyhow`](https://crates.io/crates/anyhow) — error handling in the binary (`anyhow::Result<T>` + `.context()`)
- [`ctrlc`](https://crates.io/crates/ctrlc) — SIGINT handler for temp file cleanup on Ctrl-C
- [`tracing`](https://crates.io/crates/tracing) — structured internal logging (debug/warn/error spans)
- [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) — subscriber wiring in `main` (controlled via `RUST_LOG`)

Note: user-facing terminal output (the transcript, status lines) uses `println!`/`eprintln!` — that is intentional UI output. `tracing` is for internal observability only.

### Rust (phase 2 additions)
- [`toml`](https://crates.io/crates/toml) — parse prompt TOML files

### Python
- `openai-whisper` — transcription (installed in user environment)
- `ollama` — LLM client (phase 2)
- Standard library: `argparse`, `sys`, `pathlib`, `logging`, `tomllib` (Python 3.11+)
