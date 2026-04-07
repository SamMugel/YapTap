"""Tests for src.core.llm module."""
from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch


class TestStreamResponse(unittest.TestCase):

    def test_stream_response_yields_tokens_on_valid_input(self) -> None:
        """Happy path: valid transcript + prompt file yields non-empty token chunks.

        What is being tested:
            stream_response() with a real TOML file and a mocked ollama.chat
            that returns a two-chunk streaming response.

        Why this test matters:
            Ensures the core happy-path pipeline (load TOML, build messages,
            call ollama, yield chunks) works end-to-end so that basic LLM
            output is never silently broken by a refactor.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        try:
            mock_response = [
                {"message": {"content": "Hello"}},
                {"message": {"content": " world"}},
            ]
            with patch("ollama.chat") as mock_chat:
                mock_chat.return_value = iter(mock_response)

                from src.core.llm import stream_response

                chunks = list(stream_response("some transcript", toml_path))

            self.assertEqual(chunks, ["Hello", " world"])
        finally:
            os.unlink(toml_path)

    def test_stream_response_raises_on_empty_prompt_path(self) -> None:
        """Empty string prompt_path raises ValueError before any file I/O.

        What is being tested:
            The guard clause at the top of stream_response() that rejects an
            empty prompt_path string.

        Why this test matters:
            Prevents a confusing FileNotFoundError (or silent TOML failure)
            when the Rust binary passes an uninitialised path argument.
        """
        from src.core.llm import stream_response

        with self.assertRaises(ValueError):
            list(stream_response("transcript", ""))

    def test_stream_response_raises_on_missing_prompt_file(self) -> None:
        """Non-existent prompt file path raises ValueError.

        What is being tested:
            The path.exists() check in stream_response() when the file is
            absent from the filesystem.

        Why this test matters:
            Gives a clear, actionable error instead of a raw Python
            FileNotFoundError if the prompt TOML is accidentally deleted or
            mis-configured.
        """
        from src.core.llm import stream_response

        with self.assertRaises(ValueError):
            list(stream_response("transcript", "/nonexistent/path/prompt.toml"))

    def test_stream_response_raises_on_invalid_toml(self) -> None:
        """Unparse-able TOML content raises ValueError.

        What is being tested:
            The tomllib.TOMLDecodeError handler in stream_response() when the
            prompt file contains malformed TOML.

        Why this test matters:
            Surfaces a clear ValueError (not an internal tomllib traceback)
            when a prompt file is edited incorrectly, making the failure easy
            to diagnose in production logs.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b"not valid toml !!!")
            toml_path = f.name

        try:
            from src.core.llm import stream_response

            with self.assertRaises(ValueError):
                list(stream_response("transcript", toml_path))
        finally:
            os.unlink(toml_path)

    def test_stream_response_raises_on_missing_system_field(self) -> None:
        """TOML file without a 'system' field raises ValueError.

        What is being tested:
            The 'system' key presence check in stream_response() after the
            TOML file is successfully parsed.

        Why this test matters:
            Prevents a silent KeyError or an LLM call with no system prompt
            when a prompt file is written without the required 'system' field.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\n')
            toml_path = f.name

        try:
            from src.core.llm import stream_response

            with self.assertRaises(ValueError):
                list(stream_response("transcript", toml_path))
        finally:
            os.unlink(toml_path)

    def test_stream_response_raises_on_ollama_failure(self) -> None:
        """ollama.chat raising an exception causes stream_response to raise RuntimeError.

        What is being tested:
            The except-block around the ollama.chat call that wraps any
            ollama exception in a RuntimeError.

        Why this test matters:
            Ensures callers (and the Rust binary) receive a well-typed
            RuntimeError they can catch, rather than an unhandled ollama
            exception crashing the subprocess with an unexpected traceback.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        try:
            with patch("ollama.chat", side_effect=Exception("connection refused")):
                from src.core.llm import stream_response

                with self.assertRaises(RuntimeError):
                    list(stream_response("transcript", toml_path))
        finally:
            os.unlink(toml_path)


if __name__ == "__main__":
    unittest.main()
