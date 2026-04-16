# CLI Specification (Phases 1–3)

## Invocation

```
$ yaptap [--prompt <name> | --prompt-file <path>] [--model <whisper_model>] [--llm-model <model_name>] [--llm-provider <provider>]
$ yaptap --list-prompts
```

**Phase 3 note:** `yaptap` with no flags starts the menu bar app instead of the CLI recording flow. To use the CLI recording flow in phase 3+, at least one flag must be provided (e.g. `--prompt <name>`). See [menubar.md](menubar.md).

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
| User sends SIGINT (Ctrl-C) during recording | Stop recording, delete temp WAV file, print nothing, exit 130 |
| User sends SIGINT (Ctrl-C) during LLM streaming | Kill `llm.py` subprocess immediately, delete any temp WAV file, exit 130 |
| LLM generation fails | Print `error: LLM generation failed — <subprocess stderr>` to stderr, exit 1 |
| `--llm-provider compactifai` and `MULTIVERSE_IAM_API_KEY` not set | Prompt for key on stdin, append `export MULTIVERSE_IAM_API_KEY="..."` to `~/.zshrc`, use for current invocation. See [llm.md](llm.md) § API Key Management. |
| Unknown `--llm-provider` value | Print `error: unknown llm-provider '<value>' — must be 'ollama' or 'compactifai'` to stderr, exit 1 |

For prompt-specific error cases (`--prompt` not found, `--prompt-file` not found, mutually exclusive flags), see [prompts.md](prompts.md).

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
| `--llm-model <name>` | 2 | Override the LLM model name for the active provider (default: `llama3`) |
| `--llm-provider <name>` | 2 | Override the LLM provider: `ollama` or `compactifai` (default: `ollama`); overrides `llm_provider` in config |
| `--list-prompts` | 2 | List available prompts from `config/prompts/` and exit |
| `--device <index>` | 3 | Select a specific audio input device |
