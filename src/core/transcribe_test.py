"""Tests for src.core.transcribe module."""
from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from src.core.transcribe import transcribe


class TestTranscribe(unittest.TestCase):

    def test_valid_wav_returns_transcript(self) -> None:
        """Happy path: valid WAV file path returns the stripped transcript text.

        What is being tested:
            transcribe() with a real temp file and a mocked whisper.load_model
            that returns a model whose transcribe() method yields a padded string.

        Why this test matters:
            Ensures the core happy path (existence check, model load, transcribe
            call, strip) works end-to-end so that basic transcription output is
            never silently broken by a refactor.
        """
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model") as mock_load:
                mock_model = MagicMock()
                mock_model.transcribe.return_value = {"text": "  Hello world  "}
                mock_load.return_value = mock_model
                result = transcribe(wav_path)
                self.assertEqual(result, "Hello world")
        finally:
            os.unlink(wav_path)

    def test_empty_wav_path_raises_value_error(self) -> None:
        """Empty string wav_path raises ValueError before any filesystem access.

        What is being tested:
            The guard clause at the top of transcribe() that rejects an empty
            wav_path string.

        Why this test matters:
            Prevents a confusing FileNotFoundError (or silent Whisper failure)
            when the Rust binary passes an uninitialised path argument.
        """
        with self.assertRaises(ValueError):
            transcribe("")

    def test_missing_wav_file_raises_value_error(self) -> None:
        """Non-existent WAV path raises ValueError.

        What is being tested:
            The Path(wav_path).exists() check in transcribe() when the file is
            absent from the filesystem.

        Why this test matters:
            Surfaces a clear, actionable ValueError instead of a raw Whisper
            exception if the temp WAV file is accidentally deleted between
            recording and transcription.
        """
        with self.assertRaises(ValueError):
            transcribe("/nonexistent/path/to/file.wav")

    def test_model_load_failure_raises_runtime_error(self) -> None:
        """whisper.load_model() raising an exception causes transcribe() to raise RuntimeError.

        What is being tested:
            The except-block around whisper.load_model() that wraps any model
            loading failure in a RuntimeError.

        Why this test matters:
            Ensures callers (and the Rust binary) receive a well-typed
            RuntimeError they can catch, rather than an unhandled Whisper
            exception crashing the subprocess with an unexpected traceback.
        """
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model", side_effect=Exception("model not found")):
                with self.assertRaises(RuntimeError):
                    transcribe(wav_path)
        finally:
            os.unlink(wav_path)

    def test_transcription_failure_raises_runtime_error(self) -> None:
        """model.transcribe() raising an exception causes transcribe() to raise RuntimeError.

        What is being tested:
            The except-block around model.transcribe() that wraps any inference
            failure in a RuntimeError.

        Why this test matters:
            Ensures the Rust binary receives a well-typed RuntimeError for
            transcription failures rather than an opaque Whisper traceback,
            so the error message surfaced to the user is always readable.
        """
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model") as mock_load:
                mock_model = MagicMock()
                mock_model.transcribe.side_effect = Exception("transcription failed")
                mock_load.return_value = mock_model
                with self.assertRaises(RuntimeError):
                    transcribe(wav_path)
        finally:
            os.unlink(wav_path)


if __name__ == "__main__":
    unittest.main()
