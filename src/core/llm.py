#!/usr/bin/env python3
"""LLM entry point invoked as a subprocess by the Rust binary. Supports ollama (local) and CompactifAI (cloud) providers.

Flags:
    --prompt-file   Absolute path to a prompt TOML file (required).
    --model         Model name for the active provider (default: llama3).
    --provider      LLM provider: 'ollama' or 'compactifai' (default: ollama).
    --log-dir       Directory for LLM call log files (created if absent). If omitted, no log is written.

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
        RuntimeError: If the provider fails to generate a response, if the
            provider returns zero tokens, or if provider is 'compactifai'
            and MULTIVERSE_IAM_API_KEY is not set.
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

    if not transcript.strip():
        logger.warning(
            "Empty transcript passed to LLM; model response may be unhelpful",
            extra={"provider": provider, "model": model_name},
        )

    if provider == "ollama":
        logger.debug("Calling ollama", extra={"model": model_name})
        try:
            response = ollama.chat(model=model_name, messages=messages, stream=True)
            yielded_any = False
            for chunk in response:
                token = chunk.message.content
                if token:
                    yielded_any = True
                yield token
            if not yielded_any:
                raise RuntimeError(
                    "LLM returned empty response — transcript may be empty or model unresponsive"
                )
        except RuntimeError:
            raise
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
            yielded_any = False
            for chunk in response:
                token = chunk.choices[0].delta.content or ""
                if token:
                    yielded_any = True
                yield token
            if not yielded_any:
                raise RuntimeError(
                    "LLM returned empty response — transcript may be empty or model unresponsive"
                )
        except RuntimeError:
            raise
        except openai.OpenAIError as err:
            raise RuntimeError(
                f"CompactifAI generation failed with model {model_name!r}"
            ) from err
    else:
        raise ValueError(
            f"Unknown provider: {provider!r}. Must be 'ollama' or 'compactifai'."
        )


def _write_llm_log(
    log_dir: str,
    provider: str,
    model: str,
    prompt_path: str,
    system_prompt: str,
    transcript: str,
    response: str,
    error: str | None,
) -> None:
    """Append a JSON call record to <log_dir>/llm_calls.jsonl."""
    import json
    from datetime import datetime, timezone
    from pathlib import Path

    log_path = Path(log_dir)
    try:
        log_path.mkdir(parents=True, exist_ok=True)
        record = {
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "provider": provider,
            "model": model,
            "prompt_path": prompt_path,
            "system_prompt": system_prompt,
            "transcript": transcript,
            "response": response,
            "error": error,
        }
        with (log_path / "llm_calls.jsonl").open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(record, ensure_ascii=False) + "\n")
    except Exception as exc:  # log errors must never crash the pipeline
        logger.warning(
            "Failed to write LLM log", extra={"log_dir": log_dir, "error": str(exc)}
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
    parser.add_argument(
        "--log-dir",
        default=None,
        help="Directory for LLM call log files (created if absent). If omitted, no log is written.",
    )
    args = parser.parse_args()
    transcript = sys.stdin.read()

    response_parts: list[str] = []
    try:
        for token in stream_response(transcript, args.prompt_file, args.model, args.provider):
            print(token, end="", flush=True)
            response_parts.append(token)
    except (ValueError, RuntimeError) as err:
        if args.log_dir:
            _write_llm_log(
                args.log_dir,
                args.provider,
                args.model,
                args.prompt_file,
                "",
                transcript,
                "",
                str(err),
            )
        print(f"error: {err}", file=sys.stderr)
        sys.exit(1)

    if args.log_dir:
        _write_llm_log(
            args.log_dir,
            args.provider,
            args.model,
            args.prompt_file,
            "",
            transcript,
            "".join(response_parts),
            None,
        )
    print()  # final newline


if __name__ == "__main__":
    main()
