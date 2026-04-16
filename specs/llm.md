# LLM Integration (Phases 2+)

## Responsibility

LLM inference is handled by a thin Python module (`src/core/llm.py`) invoked as a subprocess by the Rust binary. The script's only job is: receive a transcript via stdin + a prompt file path → call ollama → stream the LLM response to stdout.

---

## Interface

### Invocation (from Rust)

```
python3 src/core/llm.py --prompt-file <path> [--model <model_name>] [--provider <provider>]
```

The transcript is passed via **stdin** (UTF-8 text, newline-terminated).

> **Phase 3 note:** When running inside the `.app` bundle the interpreter is `~/.config/yaptap/.venv/bin/python` and the script path is `<resources>/scripts/llm.py`. See [packaging.md](packaging.md) for path resolution and first-launch venv setup.

| Argument | Required | Default | Description |
|---|---|---|---|
| `--prompt-file <path>` | yes | — | Absolute path to a prompt TOML file |
| `--model <name>` | no | `llama3` | Model name for the active provider |
| `--provider <name>` | no | `ollama` | LLM provider: `ollama` or `compactifai` |

**Flag translation:** the user-facing Rust CLI flags are `--llm-model` and `--llm-provider`; Rust maps them to `--model` and `--provider` when spawning the subprocess. The Python script only knows `--model` and `--provider`.

**Environment:** when `--provider compactifai` is used, Rust injects `MULTIVERSE_IAM_API_KEY` into the subprocess environment before spawning (see § API Key Management). The Python script reads the key directly from the environment.

### Output

- **stdout:** LLM response, streamed to stdout as tokens arrive. Each chunk is printed immediately (no buffering). Ends with a final `\n`.
- **stderr:** Any warnings or errors from ollama, forwarded as-is.
- **exit code:** 0 on success, 1 on any failure.

---

## Module: `src/core/llm.py`

```python
#!/usr/bin/env python3
"""LLM entry point invoked as a subprocess by the Rust binary.

Supports two providers:
  - "ollama"      — local inference via the ollama Python client
  - "compactifai" — cloud inference via the CompactifAI API (OpenAI-compatible)

Public API:
    stream_response(transcript, prompt_path, model_name, provider) -> Iterator[str]
    main() — CLI entry point; streams LLM output to stdout.
"""
from __future__ import annotations

import argparse
import logging
import os
import sys
import tomllib
from collections.abc import Iterator
from pathlib import Path

import ollama
import openai

logger = logging.getLogger(__name__)


# INTENT (for AI):
# - Purpose:   Load a prompt TOML file, combine it with a transcript,
#              route to the selected provider, and stream response tokens.
# - Invariants: prompt_path must point to a readable TOML file with 'system' field.
#               transcript may be empty (provider still returns a response).
#               Yields str chunks; never yields None.
#               When provider == "compactifai", MULTIVERSE_IAM_API_KEY must be set
#               in the environment (injected by the Rust binary before spawning).
# - Used by:   main() (CLI entry point called by the Rust binary)
# - Safe to refactor: Yes — callers depend only on the Iterator[str] yield type.
def stream_response(
    transcript: str,
    prompt_path: str,
    model_name: str = "llama3",
    provider: str = "ollama",
) -> Iterator[str]:
    """Load prompt, combine with transcript, stream LLM response.

    Args:
        transcript: The Whisper transcript text.
        prompt_path: Absolute path to the prompt TOML file.
        model_name: Model name for the active provider. Defaults to "llama3".
        provider: LLM provider — "ollama" or "compactifai". Defaults to "ollama".

    Yields:
        str chunks of the LLM response as they arrive.

    Raises:
        ValueError: If prompt_path is empty, file does not exist, TOML is invalid,
                    or provider is unknown.
        RuntimeError: If the provider fails to generate a response, or if
                      MULTIVERSE_IAM_API_KEY is not set when provider is "compactifai".
    """
    if not prompt_path:
        raise ValueError("prompt_path must not be empty")
    path = Path(prompt_path)
    if not path.exists():
        raise ValueError(f"Prompt file not found: {prompt_path!r}")

    try:
        prompt = tomllib.loads(path.read_text())
    except Exception as err:
        raise ValueError(f"Prompt file is not valid TOML: {prompt_path!r}") from err

    if "system" not in prompt:
        raise ValueError(f"Prompt file missing 'system' field: {prompt_path!r}")

    messages = [
        {"role": "system", "content": prompt["system"]},
        {"role": "user", "content": transcript},
    ]

    if provider == "ollama":
        logger.debug("Calling ollama", extra={"model": model_name})
        try:
            response = ollama.chat(model=model_name, messages=messages, stream=True)
            for chunk in response:
                token = chunk["message"]["content"]
                yield token
        except Exception as err:
            raise RuntimeError(f"Ollama generation failed with model {model_name!r}") from err

    elif provider == "compactifai":
        api_key = os.environ.get("MULTIVERSE_IAM_API_KEY")
        if not api_key:
            raise RuntimeError("MULTIVERSE_IAM_API_KEY environment variable not set")
        logger.debug("Calling CompactifAI", extra={"model": model_name})
        try:
            client = openai.OpenAI(
                base_url="https://api.compactif.ai/v1",
                api_key=api_key,
            )
            response = client.chat.completions.create(
                model=model_name,
                messages=messages,
                stream=True,
            )
            for chunk in response:
                token = chunk.choices[0].delta.content or ""
                yield token
        except Exception as err:
            raise RuntimeError(
                f"CompactifAI generation failed with model {model_name!r}"
            ) from err

    else:
        raise ValueError(f"Unknown provider: {provider!r}. Must be 'ollama' or 'compactifai'.")


def main() -> None:
    """CLI entry point: read transcript from stdin, stream LLM response to stdout."""
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)

    parser = argparse.ArgumentParser(
        description="Stream an LLM response and print to stdout."
    )
    parser.add_argument(
        "--prompt-file", required=True, help="Absolute path to a prompt TOML file"
    )
    parser.add_argument(
        "--model", default="llama3", help="Model name for the active provider (default: llama3)"
    )
    parser.add_argument(
        "--provider", default="ollama",
        help="LLM provider: 'ollama' or 'compactifai' (default: ollama)"
    )
    args = parser.parse_args()

    transcript = sys.stdin.read()

    try:
        for token in stream_response(transcript, args.prompt_file, args.model, args.provider):
            print(token, end="", flush=True)
    except (ValueError, RuntimeError) as err:
        print(f"error: {err}", file=sys.stderr)
        sys.exit(1)

    print()  # final newline


if __name__ == "__main__":
    main()
```

---

## Prompt + Transcript Assembly

The `system` field from the prompt TOML is used as the system message. The transcript is sent as the user message. No additional wrapping or prefixes are added. This message format is identical for both providers.

```python
messages = [
    {"role": "system", "content": prompt["system"]},
    {"role": "user", "content": transcript},
]
```

---

## Provider Routing

`stream_response` dispatches to the appropriate client based on `provider`:

| Provider | Client | Base URL | Auth |
|---|---|---|---|
| `ollama` | `ollama.chat(stream=True)` | local (`localhost:11434`) | none |
| `compactifai` | `openai.OpenAI(base_url=...).chat.completions.create(stream=True)` | `https://api.compactif.ai/v1` | `Authorization: Bearer <MULTIVERSE_IAM_API_KEY>` |

The CompactifAI API is OpenAI-compatible; the `openai` Python package is used as the client.

---

## Streaming

`llm.py` streams tokens to stdout as they arrive:

```python
for chunk in response:
    token = chunk["message"]["content"]
    print(token, end="", flush=True)
print()  # final newline
```

The Rust binary reads the subprocess stdout chunk-by-chunk (byte buffer, not line-by-line) and writes each chunk to the terminal immediately. Reading line-by-line would buffer the entire response until a newline, defeating streaming.

---

## Test Module: `src/core/llm_test.py`

Co-located per project architecture rules. Run with:
```
python -m unittest src.core.llm_test
```

Tests must cover:

| Test method | What | Why |
|---|---|---|
| `test_stream_response_yields_tokens_on_valid_input` | Happy path (ollama): valid transcript + prompt → non-empty chunks | Core contract of the pipeline |
| `test_stream_response_raises_on_empty_prompt_path` | `ValueError` on empty `prompt_path` | Guards against missing flag |
| `test_stream_response_raises_on_missing_prompt_file` | `ValueError` when prompt file does not exist | Guards against stale paths |
| `test_stream_response_raises_on_invalid_toml` | `ValueError` when TOML cannot be parsed | Bad user-edited prompt file |
| `test_stream_response_raises_on_missing_system_field` | `ValueError` when `system` field absent in TOML | Incomplete prompt file |
| `test_stream_response_raises_on_ollama_failure` | `RuntimeError` when ollama raises | Model not running / network issue |
| `test_stream_response_compactifai_yields_tokens` | Happy path (compactifai): `openai` client called, ollama not called, tokens yielded | Provider routing correctness |
| `test_stream_response_compactifai_raises_on_missing_api_key` | `RuntimeError` when `MULTIVERSE_IAM_API_KEY` not in env | Key injection contract |
| `test_stream_response_compactifai_raises_on_api_failure` | `RuntimeError` when `openai` client raises | Network / API error handling |
| `test_stream_response_raises_on_unknown_provider` | `ValueError` for unknown `provider` string | Guard against config typos |

All external dependencies (`ollama.chat`, `openai.OpenAI`) must be mocked via `unittest.mock.patch`. No real servers required for tests.

---

## Ollama Model Selection

| Model | Description | Recommended use |
|---|---|---|
| `llama3` | Meta Llama 3 8B | **Ollama default** — good balance of speed and quality |
| `mistral` | Mistral 7B | Fast, good for short-form rewrites |
| `llama3:70b` | Meta Llama 3 70B | High quality, requires significant RAM |
| `phi3` | Microsoft Phi-3 | Very fast, smaller context window |

The model must be pulled via `ollama pull <model>` before use.

---

## CompactifAI Model Selection

| Model | Description | Recommended use |
|---|---|---|
| `cai-llama-3-1-8b-slim` | Compressed Llama 3.1 8B | **CompactifAI default** — fast and efficient |
| `cai-llama-4-scout-slim` | Compressed Llama 4 Scout | Newer architecture, good quality |
| `mistral-small-3-1` | Mistral Small 3.1 | Good balance of speed and quality |
| `gpt-oss-20b` | 20B open-source model | High quality, larger context |

When switching `llm_provider` to `"compactifai"` in the config, also update `llm_model` to a valid CompactifAI model name. Sending an Ollama model name to the CompactifAI API (e.g. `llama3`) will return an API error.

---

## Prerequisites

**Phase 2 (CLI / development):** install Python packages manually:

```
pip install ollama openai
```

For `ollama` provider, Ollama itself must also be installed and running:

```
brew install ollama
ollama serve        # start the local server
ollama pull llama3  # download the default model
```

For `compactifai` provider, `MULTIVERSE_IAM_API_KEY` must be available in the environment (see § API Key Management).

**Phase 3 (`.app` bundle):** both `ollama` and `openai` Python packages are installed automatically into `~/.config/yaptap/.venv/` on first launch. The Ollama application/server must still be running separately when using the `ollama` provider — the app checks reachability before invoking `llm.py` (see [packaging.md](packaging.md)).

---

## API Key Management

`MULTIVERSE_IAM_API_KEY` is required when `llm_provider = "compactifai"`. Rust is responsible for obtaining and injecting the key before spawning `llm.py`. Python only reads the key from the environment.

### CLI mode

1. Rust checks the `MULTIVERSE_IAM_API_KEY` environment variable.
2. If not set, Rust prompts on stdout:
   ```
   CompactifAI API key not found.
   Enter your MULTIVERSE_IAM_API_KEY:
   ```
3. Rust reads the key from stdin.
4. Rust appends `export MULTIVERSE_IAM_API_KEY="<key>"` to `~/.zshrc`.
5. Rust prints: `Key saved to ~/.zshrc. Run 'source ~/.zshrc' or open a new terminal to apply globally.`
6. Rust sets the key in the subprocess environment for the current invocation.

### Menu bar mode (Phase 3)

1. Rust checks `MULTIVERSE_IAM_API_KEY` environment variable.
2. If not in env, Rust checks macOS Keychain (service: `yaptap`, account: `MULTIVERSE_IAM_API_KEY`).
3. If not in Keychain, Rust shows a macOS dialog:
   - Title: **"CompactifAI API Key Required"**
   - Message: *"YapTap needs your CompactifAI API key to use the CompactifAI provider."*
   - Text field for key entry (secure input)
   - Buttons: **Save** / **Cancel**
4. On **Save**: Rust stores the key in macOS Keychain and proceeds.
5. On **Cancel**: Rust shows an alert (*"CompactifAI provider requires an API key. Recording cancelled."*) and returns to idle.
6. Rust injects the key as `MULTIVERSE_IAM_API_KEY` in the `llm.py` subprocess environment.

---

## Error Handling (Rust side)

The Rust binary checks:

1. Provider packages available — validated implicitly; subprocess exit code surfaces the error.
2. `MULTIVERSE_IAM_API_KEY` available when provider is `compactifai` — Rust ensures this before spawning (see § API Key Management).
3. Subprocess exit code — if non-zero, print `error: LLM generation failed — <subprocess stderr>` to stderr, exit 1.
4. Empty stdout — treated as an empty LLM response; print as blank line.
5. SIGINT during streaming — the `ctrlc` handler kills the `llm.py` subprocess (via the `Child` handle), deletes any remaining temp WAV file, and exits 130.

---

## Error Cases (Python side)

| Condition | Behaviour |
|---|---|
| `--prompt-file` missing or file absent | `ValueError` raised, printed to stderr, exit 1 |
| TOML parse error | `ValueError` raised, printed to stderr, exit 1 |
| `system` field missing in TOML | `ValueError` raised, printed to stderr, exit 1 |
| Unknown `--provider` value | `ValueError` raised, printed to stderr, exit 1 |
| Ollama not running / connection refused | `RuntimeError` raised, printed to stderr, exit 1 |
| Model not found in ollama | `RuntimeError` raised, printed to stderr, exit 1 |
| `--provider compactifai` and `MULTIVERSE_IAM_API_KEY` not in env | `RuntimeError` raised, printed to stderr, exit 1 (Rust should have injected it; this is a bug guard) |
| CompactifAI API error / network failure | `RuntimeError` raised, printed to stderr, exit 1 |
| CompactifAI model name not recognised by API | `RuntimeError` raised, printed to stderr, exit 1 |
| Empty transcript | Still call the provider; it returns a response (may be minimal) |
