# CLI Specification (Phases 1 & 2)

## Invocation

```
$ yaptap [--prompt <name> | --prompt-file <path>] [--model <whisper_model>] [--llm-model <ollama_model>]
$ yaptap --list-prompts
```

No flags are required. Without `--prompt` or `--prompt-file`, behaviour is phase 1: raw transcript to stdout.

---

## User Experience

### Phase 1 — transcript only

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

### Phase 2 — transcript + LLM

```
$ yaptap --prompt email-reply
Recording... (press Enter to stop)
▐ 0:07

[user presses Enter]

Transcribing...
Thinking...

I hope this email finds you well. Regarding the project timeline you asked about...
```

Steps 1–4 are identical to phase 1. After transcription:

5. `Thinking...` is printed.
6. `llm.py` is spawned with the transcript piped to stdin.
7. LLM tokens are streamed and echoed to stdout as they arrive.
8. A final newline is printed after the last token.
9. The process exits 0.

### Utility commands

```
$ yaptap --list-prompts
Available prompts (config/prompts/):
  action-items    Extract action items from a voice note as a bullet list
  email-reply     Rewrite a voice note as a professional email reply
  journal         Clean up a voice journal entry into polished prose
  meeting-notes   Structure a voice brain-dump into clean meeting notes
  slack-message   Rewrite a voice note as a clear, casual Slack message
```

---

## Error Cases

| Condition | Behaviour |
|---|---|
| No microphone found | Print `error: no input device found` to stderr, exit 1 |
| `python3` not on PATH | Print `error: python3 not found` to stderr, exit 1 |
| Whisper not installed / transcription fails | Print `error: transcription failed — <subprocess stderr>` to stderr, exit 1 |
| Recording produces empty audio (silence) | Still run transcription; Whisper returns an empty string or short filler; print as-is |
| User sends SIGINT (Ctrl-C) | Stop recording, delete temp file, print nothing, exit 130 |
| `--prompt <name>` and prompt not found | Print `error: prompt '<name>' not found in config/prompts/` to stderr, exit 1 |
| `--prompt-file <path>` and file not found | Print `error: prompt file not found: <path>` to stderr, exit 1 |
| `--prompt` and `--prompt-file` both given | Print `error: --prompt and --prompt-file are mutually exclusive` to stderr, exit 1 |
| LLM generation fails | Print `error: LLM generation failed — <subprocess stderr>` to stderr, exit 1 |

---

## Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Runtime error (device, subprocess, I/O) |
| 130 | Interrupted by SIGINT |

---

## Flags

| Flag | Phase | Description |
|---|---|---|
| `--prompt <name>` | 2 | Select a named prompt from `config/prompts/` |
| `--prompt-file <path>` | 2 | Use a custom prompt file (TOML format) |
| `--model <name>` | 2 | Override the Whisper model (default: `base`) |
| `--llm-model <name>` | 2 | Override the ollama model (default: `llama3`) |
| `--list-prompts` | 2 | List available prompts from `config/prompts/` and exit |
| `--device <index>` | 3 | Select a specific audio input device |
