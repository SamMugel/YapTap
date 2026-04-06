# AGENT.md — YapTap Build & Run Guide

## Project Structure
- `src/main.rs` — Rust CLI entry point (cpal audio capture, WAV encoding, subprocess invocation)
- `src/core/transcribe.py` — Python Whisper transcription module
- `src/core/transcribe_test.py` — Python unit tests
- `src/__init__.py`, `src/core/__init__.py` — Python package markers
- `Cargo.toml` — Rust manifest (cpal, hound, tempfile, anyhow, ctrlc, tracing)
- `PRD/PRD_1.json` — Phase 1 task tracking

## Build Commands
```bash
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
```

## Run
```bash
# After cargo install --path .
yaptap
# Records until Enter, then transcribes via Whisper
```

## Prerequisites
- `python3` must be on PATH
- `ffmpeg` must be on PATH
- `openai-whisper` Python package installed
- macOS (cpal uses CoreAudio)

## Key Notes
- Python tests mock `whisper.load_model` — no real model needed for tests
- Rust resamples to 16kHz mono i16 internally (nearest-neighbour) before WAV write
- Temp WAVs written to `$TMPDIR/yaptap_<timestamp>.wav`, deleted after transcription
- SIGINT exits with code 130 and cleans up temp files
