# Audio Recording

## Responsibility

The Rust binary owns the full audio capture lifecycle: device selection, sample collection, WAV encoding, and temp file management. No audio data leaves the Rust process until it is written to a temp file.

---

## Device Selection

- Use the system default input device via `cpal::default_host().default_input_device()`.
- If no input device is available, print an error to stderr and exit 1.
- Phase 3 will introduce `--device` for explicit selection.

---

## Sample Format

| Parameter | Value | Reason |
|---|---|---|
| Sample rate | 16 000 Hz | Whisper's native rate; avoids resampling |
| Channels | 1 (mono) | Whisper expects mono; halves buffer size |
| Sample format | `i16` (16-bit signed PCM) | Universally supported; WAV default |

If the device does not support 16 kHz mono natively, `cpal` should be configured to request the closest supported config and the WAV file should record whatever rate was negotiated — Whisper handles other sample rates, though 16 kHz is optimal.

---

## Capture Lifecycle

```
launch
  │
  ▼
open cpal input stream (non-blocking callback)
  │
  ▼
push PCM samples into Vec<i16> in memory
  │
  ▼
[Enter key detected on stdin]
  │
  ▼
close input stream
  │
  ▼
encode Vec<i16> → temp .wav (hound)
  │
  ▼
return temp file path to caller
```

---

## Enter Detection

- Spawn a dedicated thread that calls `stdin.read_line()`.
- When it returns (user pressed Enter), send a message over a `std::sync::mpsc` channel to the audio thread to stop capture.
- This avoids blocking the audio callback thread on stdin.

---

## Temp File

- Created with `tempfile::Builder` in the system temp directory.
- Named `yaptap_<timestamp>.wav` for easier debugging.
- Deleted by the Rust process after the transcript has been printed (or on any error/signal).
- On SIGINT the file is deleted in a `ctrlc` handler before exit.

---

## WAV Encoding

- Use `hound::WavWriter` with the spec matching the captured format.
- Write all samples in a single pass after recording stops (buffered in memory).
- The file is closed (flushed) before the Python subprocess is spawned.

---

## Memory Considerations

- A 60-second recording at 16 kHz mono i16 = ~1.9 MB. Holding this in a `Vec<i16>` is acceptable.
- No upper bound is enforced in phase 1. A future `--max-duration` flag may truncate at a configurable limit.
