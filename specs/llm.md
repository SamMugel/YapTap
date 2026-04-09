# LLM Integration (Phase 2)

## Responsibility

LLM inference is handled by a thin Python module (`src/core/llm.py`) invoked as a subprocess by the Rust binary. The script's only job is: receive a transcript via stdin + a prompt file path → call ollama → stream the LLM response to stdout.

---

## Interface

### Invocation (from Rust)

```
python3 src/core/llm.py --prompt-file <path> [--model <model_name>]
```

The transcript is passed via **stdin** (UTF-8 text, newline-terminated).

| Argument | Required | Default | Description |
|---|---|---|---|
| `--prompt-file <path>` | yes | — | Absolute path to a prompt TOML file |
| `--model <name>` | no | `llama3` | Ollama model name |

**Flag translation:** the user-facing Rust CLI flag is `--llm-model`; Rust maps it to `--model` when spawning the subprocess. The Python script only knows `--model`.

### Output

- **stdout:** LLM response, streamed to stdout as tokens arrive. Each chunk is printed immediately (no buffering). Ends with a final `\n`.
- **stderr:** Any warnings or errors from ollama, forwarded as-is.
- **exit code:** 0 on success, 1 on any failure.

---

## Module: `src/core/llm.py`

```python
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
from pathlib import Path

import tomllib
import ollama

logger = logging.getLogger(__name__)


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
):
    """Load prompt, combine with transcript, stream ollama response.

    Args:
        transcript: The Whisper transcript text.
        prompt_path: Absolute path to the prompt TOML file.
        model_name: Ollama model name. Defaults to "llama3".

    Yields:
        str chunks of the LLM response as they arrive.

    Raises:
        ValueError: If prompt_path is empty, file does not exist, or TOML is invalid.
        RuntimeError: If ollama fails to generate a response.
    """
    ...


def main() -> None:
    """CLI entry point: read transcript from stdin, stream LLM response to stdout."""
    ...


if __name__ == "__main__":
    main()
```

---

## Prompt + Transcript Assembly

The `system` field from the prompt TOML is used as the ollama system message. The transcript is sent as the user message. No additional wrapping or prefixes are added.

```python
messages = [
    {"role": "system", "content": prompt["system"]},
    {"role": "user", "content": transcript},
]
response = ollama.chat(model=model_name, messages=messages, stream=True)
```

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
| `test_stream_response_yields_tokens_on_valid_input` | Happy path: valid transcript + prompt → non-empty chunks | Core contract of the pipeline |
| `test_stream_response_raises_on_empty_prompt_path` | `ValueError` on empty `prompt_path` | Guards against missing flag |
| `test_stream_response_raises_on_missing_prompt_file` | `ValueError` when prompt file does not exist | Guards against stale paths |
| `test_stream_response_raises_on_invalid_toml` | `ValueError` when TOML cannot be parsed | Bad user-edited prompt file |
| `test_stream_response_raises_on_missing_system_field` | `ValueError` when `system` field absent in TOML | Incomplete prompt file |
| `test_stream_response_raises_on_ollama_failure` | `RuntimeError` when ollama raises | Model not running / network issue |

All external dependencies (`ollama.chat`) must be mocked via `unittest.mock.patch`. No real ollama server required for tests.

---

## Ollama Model Selection

| Model | Description | Recommended use |
|---|---|---|
| `llama3` | Meta Llama 3 8B | **Phase 2 default** — good balance of speed and quality |
| `mistral` | Mistral 7B | Fast, good for short-form rewrites |
| `llama3:70b` | Meta Llama 3 70B | High quality, requires significant RAM |
| `phi3` | Microsoft Phi-3 | Very fast, smaller context window |

Phase 2 defaults to `llama3`. The model must be pulled via `ollama pull <model>` before use.

---

## Prerequisites

The user must have ollama installed and running:

```
brew install ollama
ollama serve        # start the local server
ollama pull llama3  # download the default model
```

The `ollama` Python package must be installed:

```
pip install ollama
```

---

## Error Handling (Rust side)

The Rust binary checks:

1. `ollama` Python package available — validated implicitly; subprocess exit code surfaces the error.
2. Subprocess exit code — if non-zero, print `error: LLM generation failed — <subprocess stderr>` to stderr, exit 1.
3. Empty stdout — treated as an empty LLM response; print as blank line.
4. SIGINT during streaming — the `ctrlc` handler kills the `llm.py` subprocess (via the `Child` handle), deletes any remaining temp WAV file, and exits 130.

---

## Error Cases (Python side)

| Condition | Behaviour |
|---|---|
| `--prompt-file` missing or file absent | `ValueError` raised, printed to stderr, exit 1 |
| TOML parse error | `ValueError` raised, printed to stderr, exit 1 |
| `system` field missing in TOML | `ValueError` raised, printed to stderr, exit 1 |
| Ollama not running / connection refused | `RuntimeError` raised, printed to stderr, exit 1 |
| Model not found in ollama | `RuntimeError` raised, printed to stderr, exit 1 |
| Empty transcript | Still call ollama; it returns a response (may be minimal) |
