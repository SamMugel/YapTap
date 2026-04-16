#!/usr/bin/env python3
"""LLM entry point invoked as a subprocess by the Rust binary. Supports ollama (local) and CompactifAI (cloud) providers.

Public API:
    stream_response(transcript, prompt_path, model_name, provider) -> Iterator[str]
    main() — CLI entry point; streams LLM output to stdout.
"""
from __future__ import annotations

import argparse
import logging
import os
import sys

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]
from pathlib import Path
from typing import Iterator

import ollama
import openai

logger = logging.getLogger(__name__)
logger.addHandler(logging.NullHandler())

__all__ = ["stream_response"]


# INTENT (for AI):
# - Purpose:   Load a prompt TOML file, combine it with a transcript,
#              and stream the response tokens from the selected provider.
#              Routes to ollama (local) or CompactifAI (cloud) based on `provider`.
# - Invariants: prompt_path must point to a readable TOML file with 'system' field.
#               transcript may be empty (provider still returns a response).
#               Yields str chunks; never yields None.
#               When provider == 'compactifai', MULTIVERSE_IAM_API_KEY must be set in the environment.
# - Used by:   main() (CLI entry point called by the Rust binary)
# - Safe to refactor: Yes — callers depend only on the Iterator[str] yield type and provider contract.
def stream_response(
    transcript: str,
    prompt_path: str,
    model_name: str = "llama3",
    provider: str = "ollama",
) -> Iterator[str]:
    """Load prompt, combine with transcript, stream provider response.

    Args:
        transcript: The Whisper transcript text.
        prompt_path: Absolute path to the prompt TOML file.
        model_name: Model name for the active provider. Defaults to "llama3".
        provider: LLM provider: 'ollama' or 'compactifai'. Defaults to 'ollama'.

    Yields:
        str chunks of the LLM response as they arrive.

    Raises:
        ValueError: If prompt_path is empty, file does not exist, TOML is
            invalid, the 'system' field is absent, or provider is unknown.
        RuntimeError: If the provider fails to generate a response, or if
            provider is 'compactifai' and MULTIVERSE_IAM_API_KEY is not set.
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

    if provider == "ollama":
        logger.debug("Calling ollama", extra={"model": model_name})
        try:
            response = ollama.chat(model=model_name, messages=messages, stream=True)
            for chunk in response:
                yield chunk["message"]["content"]
        except Exception as exc:
            raise RuntimeError(f"ollama error: {exc}") from exc
    elif provider == "compactifai":
        api_key = os.environ.get("MULTIVERSE_IAM_API_KEY")
        if not api_key:
            raise RuntimeError("MULTIVERSE_IAM_API_KEY environment variable not set")
        logger.debug("Calling CompactifAI", extra={"model": model_name})
        client = openai.OpenAI(base_url="https://api.compactif.ai/v1", api_key=api_key)
        try:
            response = client.chat.completions.create(
                model=model_name, messages=messages, stream=True
            )
            for chunk in response:
                yield chunk.choices[0].delta.content or ""
        except openai.OpenAIError as err:
            raise RuntimeError(
                f"CompactifAI generation failed with model {model_name!r}"
            ) from err
    else:
        raise ValueError(
            f"Unknown provider: {provider!r}. Must be 'ollama' or 'compactifai'."
        )


def main() -> None:
    """CLI entry point: read transcript from stdin, stream LLM response to stdout."""
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)
    parser = argparse.ArgumentParser(description="Stream an LLM response to stdout.")
    parser.add_argument(
        "--prompt-file",
        required=True,
        help="Absolute path to a prompt TOML file",
    )
    parser.add_argument(
        "--model",
        default="llama3",
        help="Model name for the active provider (default: llama3)",
    )
    parser.add_argument(
        "--provider",
        default="ollama",
        help="LLM provider: 'ollama' or 'compactifai' (default: ollama)",
    )
    args = parser.parse_args()
    transcript = sys.stdin.read()

    try:
        for chunk in stream_response(transcript, args.prompt_file, args.model, args.provider):
            print(chunk, end="", flush=True)
        print()
    except (ValueError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
