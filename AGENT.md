# AGENT.md — YapTap Build & Run Guide

## Project Structure
- `src/main.rs` — Rust CLI entry point (cpal audio capture, WAV encoding, subprocess invocation)
- `src/core/transcribe.py` — Python Whisper transcription module
- `src/core/transcribe_test.py` — Python unit tests for transcription
- `src/core/llm.py` — Python Ollama LLM module (phase 2)
- `src/core/llm_test.py` — Python unit tests for LLM (phase 2)
- `src/__init__.py`, `src/core/__init__.py` — Python package markers
- `Cargo.toml` — Rust manifest (cpal, hound, tempfile, anyhow, ctrlc, tracing, toml, dirs)
- `config/prompts/` — Bundled default prompt TOML files (phase 2)
- `PRD/PRD_1.json` — Phase 1 task tracking
- `PRD/PRD_2.json` — Phase 2 task tracking
- `src/app.rs` — Phase 3 menu bar app mode (NSApplication, TrayIcon, hotkey, pipeline)
- `src/audio.rs` — Non-blocking audio capture (AudioHandle, start_recording, stop_and_save)
- `src/config.rs` — Config file management (~/.config/yaptap/config.toml)
- `src/hotkey.rs` — Global hotkey parsing and AXIsProcessTrusted check
- `src/transcription.rs` — run_transcription() wrapper for transcribe.py subprocess
- `src/llm.rs` — run_llm_collect() wrapper for llm.py subprocess (buffers output)
- `assets/icons/` — Menu bar icon PNGs (idle/active @1x and @2x)
- `PRD/PRD_3.json` — Phase 3 task tracking

## Build Commands
```bash
# Build with all Phase 3 deps (takes longer first time due to cocoa/tray-icon)
cargo build

# Rust build
cargo build

# Rust lint (must pass -D warnings)
cargo clippy -- -D warnings

# Install binary
cargo install --path .
```

## Test Commands
```bash
# Python unit tests (run from repo root)
python -m unittest src.core.transcribe_test
python -m unittest src.core.llm_test       # phase 2
```

## Run
```bash
# Phase 3: launch menu bar app (no args)
yaptap

# Phase 3: with explicit audio device
yaptap --device 0

# After cargo install --path .
yaptap                              # phase 1: record → transcript
yaptap --list-prompts               # show available prompts from config/prompts/
yaptap --prompt email-reply         # phase 2: record → transcript → LLM
yaptap --prompt-file my-prompt.toml # phase 2: record → transcript → LLM with custom prompt
```

## Prerequisites
- `python3` must be on PATH
- `ffmpeg` must be on PATH
- `openai-whisper` Python package installed
- `ollama` Python package installed (phase 2)
- `ollama` server running with at least one model pulled (phase 2)
- macOS (cpal uses CoreAudio)
- `macOS Accessibility permission` — required for global hotkey (grant in System Settings → Privacy & Security → Accessibility)

## Debugging
```bash
# Launch app mode with all tracing output visible in the terminal
RUST_LOG=debug ./target/debug/yaptap

# Info-level only (state transitions and errors)
RUST_LOG=info ./target/debug/yaptap
```
In app mode, stderr is invisible when launched from Finder or a login item.
Always launch from a terminal with `RUST_LOG` set when diagnosing issues.

## Key Notes
- Python tests mock `whisper.load_model` and `ollama.chat` — no real model needed for tests
- Rust resamples to 16kHz mono i16 internally (nearest-neighbour) before WAV write
- Temp WAVs written to `$TMPDIR/yaptap_<timestamp>.wav`, deleted after transcription
- SIGINT exits with code 130 and cleans up temp files
- Prompts live in `config/prompts/`; add `.toml` files there to extend the prompt library
- `llm.py` reads transcript from stdin, streams response to stdout
- Phase 3 app mode: `yaptap` (no args) → menu bar icon, global hotkey ⌥Space starts/stops recording, result goes to clipboard
- Config file at `~/.config/yaptap/config.toml` (created on first launch)
- Single-instance guard via `~/.config/yaptap/yaptap.lock`
- `cargo clippy -- -D warnings` must pass; `#![allow(unexpected_cfgs)]` in app.rs suppresses objc macro noise

