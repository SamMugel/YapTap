"""Tests for src.core.transcribe module."""
from __future__ import annotations

import unittest
from unittest.mock import MagicMock, patch
from pathlib import Path
import tempfile
import os


class TestTranscribe(unittest.TestCase):

    def test_valid_wav_returns_transcript(self):
        """Valid WAV file returns the transcript text."""
        # Create a real temp file so existence check passes
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model") as mock_load:
                mock_model = MagicMock()
                mock_model.transcribe.return_value = {"text": "  Hello world  "}
                mock_load.return_value = mock_model

                from src.core.transcribe import transcribe
                result = transcribe(wav_path)
                self.assertEqual(result, "Hello world")
        finally:
            os.unlink(wav_path)

    def test_empty_wav_path_raises_value_error(self):
        from src.core.transcribe import transcribe
        with self.assertRaises(ValueError):
            transcribe("")

    def test_missing_wav_file_raises_value_error(self):
        from src.core.transcribe import transcribe
        with self.assertRaises(ValueError):
            transcribe("/nonexistent/path/to/file.wav")

    def test_model_load_failure_raises_runtime_error(self):
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model", side_effect=Exception("model not found")):
                from src.core.transcribe import transcribe
                with self.assertRaises(RuntimeError):
                    transcribe(wav_path)
        finally:
            os.unlink(wav_path)

    def test_transcription_failure_raises_runtime_error(self):
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
            wav_path = f.name
        try:
            with patch("whisper.load_model") as mock_load:
                mock_model = MagicMock()
                mock_model.transcribe.side_effect = Exception("transcription failed")
                mock_load.return_value = mock_model
                from src.core.transcribe import transcribe
                with self.assertRaises(RuntimeError):
                    transcribe(wav_path)
        finally:
            os.unlink(wav_path)


if __name__ == "__main__":
    unittest.main()
