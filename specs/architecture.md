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

`yaptap` communicates with Python scripts through a simple stdio contract. The authoritative details are in [transcription.md](transcription.md) and [llm.md](llm.md); this is a summary.

> **Phase 3 note:** Inside the `.app` bundle the scripts are at `<resources>/scripts/{transcribe,llm}.py` and the interpreter is `~/.config/yaptap/.venv/bin/python`. During development (`cargo run`) they remain at `src/core/` and `python3` is used. See [packaging.md](packaging.md).

| Script | stdin | stdout | stderr | exit code |
|---|---|---|---|---|
| `transcribe.py` | nothing (WAV path is a CLI arg) | UTF-8 transcript, newline-terminated | forwarded for debugging | 0 success / non-zero error |
| `llm.py` | UTF-8 transcript, newline-terminated | LLM response streamed token by token; final `\n` | forwarded for debugging | 0 success / 1 error |

`yaptap` propagates non-zero exit codes to the user as an error message and exits 1.

---

## Dependencies (phases 1 & 2)

### Rust
- [`cpal`](https://crates.io/crates/cpal) — cross-platform audio capture
- [`hound`](https://crates.io/crates/hound) — WAV encoding
- [`anyhow`](https://crates.io/crates/anyhow) — error handling in the binary (`anyhow::Result<T>` + `.context()`)
- [`ctrlc`](https://crates.io/crates/ctrlc) — SIGINT handler for temp file cleanup on Ctrl-C
- [`tracing`](https://crates.io/crates/tracing) — structured internal logging (debug/warn/error spans)
- [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) — subscriber wiring in `main` (controlled via `RUST_LOG`)

Note: user-facing terminal output (the transcript, status lines) uses `println!`/`eprintln!` — that is intentional UI output. `tracing` is for internal observability only.

### Rust (phase 2 additions)
- [`toml`](https://crates.io/crates/toml) — parse prompt TOML files

### System
- `ffmpeg` — required by Whisper for audio decoding; must be on `PATH`

### Python
- `openai-whisper` — transcription
- `ollama` — LLM client (phase 2)
- Standard library: `argparse`, `sys`, `pathlib`, `logging`, `tomllib` (Python 3.11+)

> **Phase 3 note:** Both packages are installed automatically into `~/.config/yaptap/.venv/` on first launch of the `.app` bundle. For CLI/development use (phases 1 & 2) they must be installed manually. See [packaging.md](packaging.md).

---

## Phase 3 Component Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                    yaptap (Rust binary)                       │
│                                                               │
│  ┌─────────────────────┐   ┌──────────────────────────────┐  │
│  │  NSApplication       │   │  CGEventTap                  │  │
│  │  (menu bar, LSUIEl.) │   │  keyDown listener (HID level)│  │
│  └──────────┬──────────┘   └──────────────┬───────────────┘  │
│             │ prompt select                │ hotkey press      │
│             ▼                             ▼                   │
│  config/prompts/         ┌────────────────────────────────┐   │
│  (TOML files)            │  State machine                  │   │
│                          │  IDLE → RECORDING → PROCESSING  │   │
│                          │                                 │   │
│                          │  [cpal audio → WAV → temp file] │   │
│                          │           │                     │   │
│                          │           ▼                     │   │
│                          │  spawn: transcribe.py           │   │
│                          │           │ (transcript)        │   │
│                          │           ▼                     │   │
│                          │  spawn: llm.py (if prompt set)  │   │
│                          │           │ (full LLM output)   │   │
│                          │           ▼                     │   │
│                          │  NSPasteboard ← output text     │   │
│                          └────────────────────────────────┘   │
│                                                               │
│  ~/.config/yaptap/config.toml  (hotkey + prompt selection)   │
│  ~/.config/yaptap/yaptap.lock  (single-instance guard)       │
└──────────────────────────────────────────────────────────────┘
```

---

## Directory Layout (phase 3 additions)

```
YapTap/
├── Makefile                      # build targets: build, icns, app, dmg, clean
├── assets/
│   ├── Info.plist                # macOS app bundle metadata (source-controlled)
│   └── icons/
│       ├── yaptap-idle.png       # menu bar icon — idle state (@1×)
│       ├── yaptap-idle@2x.png    # menu bar icon — idle state (@2×, Retina)
│       ├── yaptap-active.png     # menu bar icon — active state (@1×)
│       └── yaptap-active@2x.png  # menu bar icon — active state (@2×, Retina)
│       # AppIcon.iconset/ and YapTap.icns are generated artifacts (git-ignored)
├── dist/                         # build output (git-ignored)
│   ├── YapTap.app/               # assembled app bundle
│   └── YapTap.dmg                # distributable disk image
└── PRD/
    └── PRD_3.json               # phase 3 task tracking
```

---

## Dependencies (phase 3 additions)

### Rust
- [`tray-icon`](https://crates.io/crates/tray-icon) — macOS menu bar icon and native dropdown menu
- [`rdev`](https://crates.io/crates/rdev) — global keyboard event listener (wraps `CGEventTap` on macOS)
- [`arboard`](https://crates.io/crates/arboard) — clipboard read/write (`NSPasteboard` on macOS)
