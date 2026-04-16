#!/usr/bin/env python3
"""Ollama LLM entry point invoked as a subprocess by the Rust binary.

Public API:
    stream_response(transcript, prompt_path, model_name) -> Iterator[str]
    main() — CLI entry point; streams LLM output to stdout.
"""
from __future__ import annotations

import argparse
import logging
import sys
try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]
from pathlib import Path
from typing import Iterator

import ollama

logger = logging.getLogger(__name__)
logger.addHandler(logging.NullHandler())

__all__ = ["stream_response"]


# INTENT (for AI):
# - Purpose:   Load a prompt TOML file, combine it with a transcript,
#              send to ollama, and stream the response tokens.
# - Invariants: prompt_path must point to a readable TOML file with 'system' field.
#               transcript may be empty (ollama still returns a response).
#               Yields str chunks; never yields None.
# - Used by:   main() (CLI entry point called by the Rust binary)
# - Safe to refactor: Yes — callers depend only on the Iterator[str] yield type.
def stream_response(
    transcript: str,
    prompt_path: str,
    model_name: str = "llama3",
) -> Iterator[str]:
    """Load prompt, combine with transcript, stream ollama response.

    Args:
        transcript: The Whisper transcript text.
        prompt_path: Absolute path to the prompt TOML file.
        model_name: Ollama model name. Defaults to "llama3".

    Yields:
        str chunks of the LLM response as they arrive.

    Raises:
        ValueError: If prompt_path is empty, file does not exist, or TOML
            is invalid, or the 'system' field is absent.
        RuntimeError: If ollama fails to generate a response.
    """
    if prompt_path == "":
        raise ValueError("prompt_path must not be empty")

    path = Path(prompt_path)
    if not path.exists():
        raise ValueError(f"prompt file does not exist: {prompt_path}")

    try:
        with path.open("rb") as fh:
            prompt = tomllib.load(fh)
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"failed to parse prompt TOML: {exc}") from exc

    if "system" not in prompt:
        raise ValueError(
            f"prompt file missing required 'system' field: {prompt_path}"
        )

    messages = [
        {"role": "system", "content": prompt["system"]},
        {"role": "user", "content": transcript},
    ]

    try:
        response = ollama.chat(model=model_name, messages=messages, stream=True)
        for chunk in response:
            yield chunk["message"]["content"]
    except Exception as exc:
        raise RuntimeError(f"ollama error: {exc}") from exc


def main() -> None:
    """CLI entry point: read transcript from stdin, stream LLM response to stdout."""
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)
    parser = argparse.ArgumentParser(
        description="Stream an LLM response from ollama to stdout."
    )
    parser.add_argument(
        "--prompt-file",
        required=True,
        help="Absolute path to a prompt TOML file",
    )
    parser.add_argument(
        "--model",
        default="llama3",
        help="Ollama model name (default: llama3)",
    )
    args = parser.parse_args()
    transcript = sys.stdin.read()

    try:
        for chunk in stream_response(transcript, args.prompt_file, args.model):
            print(chunk, end="", flush=True)
        print()
    except (ValueError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
