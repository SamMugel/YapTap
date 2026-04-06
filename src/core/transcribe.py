"""
Whisper-based audio transcription module.

Invoked as a subprocess: python3 src/core/transcribe.py <wav_path>
Prints transcript to stdout on success.
Prints error: <message> to stderr and exits 1 on failure.
"""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

import whisper

log = logging.getLogger(__name__)


def transcribe(wav_path: str, model_name: str = "base") -> str:
    # INTENT (for AI):
    # Purpose: Load a Whisper model and transcribe a WAV file, returning the stripped transcript.
    # Invariants:
    #   - wav_path must be non-empty string pointing to an existing file
    #   - Returns stripped string (may be empty if recording was silent)
    #   - Raises ValueError for invalid inputs, RuntimeError for ML failures
    # Used by: main() in this module, called from Rust subprocess
    # Safe to refactor: parameter names, model loading strategy, but NOT the return type contract
    if not wav_path:
        raise ValueError("wav_path must not be empty")

    if not Path(wav_path).exists():
        raise ValueError(f"WAV file not found: {wav_path}")

    try:
        model = whisper.load_model(model_name)
    except Exception as e:
        raise RuntimeError(
            f"Failed to load Whisper model '{model_name}': {e}"
        ) from e

    try:
        result = model.transcribe(wav_path)
    except Exception as e:
        raise RuntimeError(f"Transcription failed: {e}") from e

    return result["text"].strip()


def main() -> None:
    # INTENT (for AI):
    # Purpose: CLI entry point. Parses wav_path from argv, calls transcribe(), prints result.
    # Invariants:
    #   - Exit 0 on success (prints transcript to stdout)
    #   - Exit 1 on any failure (prints "error: <msg>" to stderr)
    # Used by: Rust subprocess spawn
    # Safe to refactor: argument parsing details, but NOT the exit code contract
    parser = argparse.ArgumentParser(
        description="Transcribe a WAV file using Whisper."
    )
    parser.add_argument("wav_path", help="Path to the WAV file to transcribe.")
    args = parser.parse_args()

    try:
        transcript = transcribe(args.wav_path)
        print(transcript)
    except (ValueError, RuntimeError) as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
