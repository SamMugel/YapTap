"""Tests for src.core.llm module."""
from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from src.core.llm import stream_response


class LlmTest(unittest.TestCase):

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
                with self.assertRaises(RuntimeError):
                    list(stream_response("transcript", toml_path))
        finally:
            os.unlink(toml_path)

    def test_stream_response_compactifai_yields_tokens(self) -> None:
        """CompactifAI provider happy path: yields non-empty token chunks.

        What is being tested:
            stream_response() with provider='compactifai', a mocked openai.OpenAI
            client returning a two-chunk streaming response, and MULTIVERSE_IAM_API_KEY set.

        Why this test matters:
            Ensures the CompactifAI branch works end-to-end and that ollama is
            never called when provider='compactifai', preventing silent regression
            after a refactor.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        os.environ["MULTIVERSE_IAM_API_KEY"] = "test-key-123"
        try:
            chunk1 = MagicMock()
            chunk1.choices = [MagicMock()]
            chunk1.choices[0].delta.content = "Hello"
            chunk2 = MagicMock()
            chunk2.choices = [MagicMock()]
            chunk2.choices[0].delta.content = " world"

            mock_client = MagicMock()
            mock_client.chat.completions.create.return_value = iter([chunk1, chunk2])

            with patch("ollama.chat") as mock_ollama, patch(
                "openai.OpenAI", return_value=mock_client
            ):
                chunks = list(
                    stream_response(
                        "some transcript", toml_path, "cai-llama-3-1-8b-slim", "compactifai"
                    )
                )
                mock_ollama.assert_not_called()

            self.assertEqual(chunks, ["Hello", " world"])
        finally:
            os.environ.pop("MULTIVERSE_IAM_API_KEY", None)
            os.unlink(toml_path)

    def test_stream_response_compactifai_raises_on_missing_api_key(self) -> None:
        """CompactifAI provider raises RuntimeError when MULTIVERSE_IAM_API_KEY is absent.

        What is being tested:
            The api_key guard in the compactifai branch of stream_response()
            when MULTIVERSE_IAM_API_KEY is not present in the environment.

        Why this test matters:
            Ensures a clear RuntimeError with an actionable message is raised
            instead of an obscure openai authentication error reaching the user.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        os.environ.pop("MULTIVERSE_IAM_API_KEY", None)
        try:
            with self.assertRaises(RuntimeError) as ctx:
                list(
                    stream_response(
                        "transcript", toml_path, "cai-llama-3-1-8b-slim", "compactifai"
                    )
                )
            self.assertIn("MULTIVERSE_IAM_API_KEY", str(ctx.exception))
        finally:
            os.unlink(toml_path)

    def test_stream_response_compactifai_raises_on_api_failure(self) -> None:
        """CompactifAI provider raises RuntimeError when the openai client raises.

        What is being tested:
            The openai.OpenAIError handler in the compactifai branch that wraps
            API errors in a RuntimeError with a descriptive message.

        Why this test matters:
            Ensures callers receive a well-typed RuntimeError they can catch
            instead of an unhandled openai exception crashing the subprocess.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        os.environ["MULTIVERSE_IAM_API_KEY"] = "test-key-123"
        try:
            import openai as _openai

            mock_client = MagicMock()
            mock_client.chat.completions.create.side_effect = _openai.OpenAIError(
                "connection refused"
            )

            with patch("openai.OpenAI", return_value=mock_client):
                with self.assertRaises(RuntimeError) as ctx:
                    list(
                        stream_response(
                            "transcript",
                            toml_path,
                            "cai-llama-3-1-8b-slim",
                            "compactifai",
                        )
                    )
            self.assertIn("CompactifAI generation failed", str(ctx.exception))
        finally:
            os.environ.pop("MULTIVERSE_IAM_API_KEY", None)
            os.unlink(toml_path)

    def test_stream_response_raises_on_unknown_provider(self) -> None:
        """Unknown provider value raises ValueError with descriptive message.

        What is being tested:
            The else-branch in stream_response() that rejects any provider
            value other than 'ollama' or 'compactifai'.

        Why this test matters:
            Prevents silent no-op or confusing AttributeError when a typo or
            unsupported provider string is passed via --provider CLI flag.
        """
        with tempfile.NamedTemporaryFile(
            suffix=".toml", delete=False, mode="wb"
        ) as f:
            f.write(b'name = "test"\ndescription = "test"\nsystem = "Be helpful"\n')
            toml_path = f.name

        try:
            with self.assertRaises(ValueError) as ctx:
                list(stream_response("transcript", toml_path, "llama3", "unknown_provider"))
            self.assertIn("Unknown provider", str(ctx.exception))
        finally:
            os.unlink(toml_path)


if __name__ == "__main__":
    unittest.main()
