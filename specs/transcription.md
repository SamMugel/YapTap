# Transcription

## Responsibility

Transcription is handled by a thin Python module (`src/core/transcribe.py`) invoked as a subprocess by the Rust binary. The script's only job is: receive a WAV path → run Whisper → print the transcript to stdout.

---

## Interface

### Invocation (from Rust)

```
python3 src/core/transcribe.py <wav_path> [--model <model_name>]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<wav_path>` | yes | — | Absolute path to the temp WAV file |
| `--model` | no | `base` | Whisper model size (`tiny`, `base`, `small`, `medium`, `large`) |

### Output

- **stdout:** The transcript as a single UTF-8 string, stripped of leading/trailing whitespace, followed by `\n`.
- **stderr:** Any warnings or errors from Whisper, forwarded as-is.
- **exit code:** 0 on success, 1 on any failure.

---

## Module: `src/core/transcribe.py`

```python
#!/usr/bin/env python3
"""Whisper transcription entry point invoked as a subprocess by the Rust binary.

Public API:
    transcribe(wav_path, model_name) -> str
    main() — CLI entry point; prints transcript to stdout.
"""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

import whisper

logger = logging.getLogger(__name__)


# INTENT (for AI):
# - Purpose:   Load a Whisper model and transcribe a WAV file to text.
# - Invariants: wav_path must point to a readable WAV file.
#               Returns empty string on silence; never returns None.
# - Used by:   main() (CLI entry point called by the Rust binary)
# - Safe to refactor: Yes — callers depend only on the str return type.
def transcribe(wav_path: str, model_name: str = "base") -> str:
    """Transcribe a WAV file using Whisper and return the transcript text.

    Args:
        wav_path: Absolute path to the WAV file to transcribe.
        model_name: Whisper model size. Defaults to "base".

    Returns:
        The transcript as a stripped string. Empty string if Whisper
        produces no output.

    Raises:
        ValueError: If wav_path is empty or the file does not exist.
        RuntimeError: If Whisper fails to load the model or transcribe.
    """
    if not wav_path:
        raise ValueError("wav_path must not be empty")
    path = Path(wav_path)
    if not path.exists():
        raise ValueError(f"WAV file not found: {wav_path!r}")

    logger.debug("Loading Whisper model", extra={"model": model_name})
    try:
        model = whisper.load_model(model_name)
    except Exception as err:
        raise RuntimeError(f"Failed to load Whisper model {model_name!r}") from err

    logger.debug("Transcribing", extra={"wav_path": wav_path})
    try:
        result = model.transcribe(str(path))
    except Exception as err:
        raise RuntimeError(f"Whisper transcription failed for {wav_path!r}") from err

    return result["text"].strip()


def main() -> None:
    """CLI entry point: parse args, transcribe, print to stdout."""
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)

    parser = argparse.ArgumentParser(
        description="Transcribe a WAV file using Whisper and print the result."
    )
    parser.add_argument("wav_path", help="Absolute path to the WAV file")
    parser.add_argument("--model", default="base", help="Whisper model size (default: base)")
    args = parser.parse_args()

    try:
        transcript = transcribe(args.wav_path, args.model)
    except (ValueError, RuntimeError) as err:
        print(f"error: {err}", file=sys.stderr)
        sys.exit(1)

    print(transcript)


if __name__ == "__main__":
    main()
```

---

## Test Module: `src/core/transcribe_test.py`

Co-located per project architecture rules (`python-project-architecture.mdc`). Run with:
```
python -m unittest src.core.transcribe_test
```

Tests must cover:

| Test method | What | Why |
|---|---|---|
| `test_transcribe_returns_text_on_valid_audio` | Happy path: valid WAV → non-empty string | Core contract of the pipeline; regression here breaks all voice input |
| `test_transcribe_raises_on_empty_wav_path` | `ValueError` on empty `wav_path` | Prevents passing garbage to Whisper, which would produce a cryptic error |
| `test_transcribe_raises_on_missing_file` | `ValueError` when file does not exist | Guards against stale temp-file paths |
| `test_transcribe_raises_on_model_load_failure` | `RuntimeError` when `whisper.load_model` raises | Whisper model files can be absent; must surface a clear error |
| `test_transcribe_raises_on_transcription_failure` | `RuntimeError` when `model.transcribe` raises | Corrupt/unsupported audio must not silently produce empty output |

All external dependencies (`whisper.load_model`, `whisper.model.transcribe`) must be mocked via `unittest.mock.patch`. No real audio hardware or model weights are loaded during tests.

---

## Whisper Model Selection

| Model | Size | Speed (CPU) | Accuracy | Recommended use |
|---|---|---|---|---|
| `tiny` | 39 M params | very fast | lower | Quick testing |
| `base` | 74 M params | fast | good | **Phase 1 default** |
| `small` | 244 M params | moderate | better | Everyday use |
| `medium` | 769 M params | slow | high | When accuracy matters |
| `large` | 1550 M params | very slow | best | Not recommended on CPU |

Phase 1 defaults to `base`. The model is loaded fresh on each invocation (acceptable for phase 1; phase 3 will consider a persistent daemon).

---

## Prerequisites

The user must have Whisper installed in their Python environment:

```
pip install openai-whisper
```

`ffmpeg` must also be available on `PATH` (required by Whisper for audio decoding).

---

## Error Handling (Rust side)

The Rust binary checks:

1. `python3` is on `PATH` — if not, emit `error!(...)` via `tracing` and print `error: python3 not found` to stderr, exit 1.
2. Subprocess exit code — if non-zero, emit `error!(...)` via `tracing` and print `error: transcription failed — <subprocess stderr>` to stderr, exit 1.
3. Empty stdout — treated as a successful empty transcript (printed as blank line).
