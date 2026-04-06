# CLI Specification (Phase 1)

## Invocation

```
$ yaptap
```

No subcommands or flags are required for phase 1.

---

## User Experience

```
$ yaptap
Recording... (press Enter to stop)
▐ 0:04

[user presses Enter]

Transcribing...

She sells seashells by the seashore.
```

1. On launch, the binary immediately begins capturing audio from the default input device.
2. A single status line is printed: `Recording... (press Enter to stop)`.
3. An elapsed-time counter updates in place on the line below (overwrite with `\r`).
4. When the user presses **Enter**, recording stops and `Transcribing...` is printed.
5. Once the transcript is ready it is printed to stdout, followed by a newline.
6. The process exits 0.

---

## Error Cases

| Condition | Behaviour |
|---|---|
| No microphone found | Print `error: no input device found` to stderr, exit 1 |
| Whisper not installed / Python not found | Print `error: transcription failed — <subprocess stderr>` to stderr, exit 1 |
| Recording produces empty audio (silence) | Still run transcription; Whisper returns an empty string or short filler; print as-is |
| User sends SIGINT (Ctrl-C) | Stop recording, delete temp file, print nothing, exit 130 |

---

## Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Runtime error (device, subprocess, I/O) |
| 130 | Interrupted by SIGINT |

---

## Future Flags (not in phase 1, reserved)

| Flag | Phase | Description |
|---|---|---|
| `--prompt <name>` | 2 | Select a named prompt from the prompt library |
| `--prompt-file <path>` | 2 | Use a custom prompt file |
| `--model <name>` | 2 | Override the Whisper model (default: `base`) |
| `--device <index>` | 3 | Select a specific audio input device |
