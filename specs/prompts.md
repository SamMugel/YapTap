# Prompt System (Phase 2)

## Overview

A prompt is a TOML file that provides the LLM with instructions on how to transform a voice transcript. The user selects a prompt at invocation time; `yaptap` concatenates the prompt's `system` field with the transcript and sends them to the LLM.

---

## Prompt File Format

Each prompt is a `.toml` file with three required fields:

```toml
name = "Email Reply"
description = "Rewrite a voice note as a professional email reply"
system = """
You are a professional email writer. ...
"""
```

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Human-readable display name (shown in `--list-prompts`) |
| `description` | string | yes | One-line description (shown in `--list-prompts`) |
| `system` | string | yes | The full system prompt passed to the LLM |

The `system` field is passed as the system message to the LLM. The transcript is passed as the user message.

---

## Prompt Directory

Prompts live in `config/prompts/` relative to the project root:

```
config/prompts/
```

This directory is the single source of truth for all prompts. Users add or edit `.toml` files here directly.

---

## Bundled Prompts

| Filename | Name | Description |
|---|---|---|
| `email-reply.toml` | Email Reply | Rewrite a voice note as a professional email reply |
| `meeting-notes.toml` | Meeting Notes | Structure a voice brain-dump into clean meeting notes |
| `slack-message.toml` | Slack Message | Rewrite a voice note as a clear, casual Slack message |
| `action-items.toml` | Action Items | Extract action items from a voice note as a bullet list |
| `journal.toml` | Journal | Clean up a voice journal entry into polished prose |

---

## Prompt Lookup

When `--prompt <name>` is passed, `yaptap` resolves the prompt file as follows:

1. Look for `config/prompts/<name>.toml`.
2. If not found, print `error: prompt '<name>' not found in config/prompts/` to stderr, exit 1.

When `--prompt-file <path>` is passed:

1. Use the file at `<path>` directly (absolute or relative to CWD).
2. If the file does not exist, print `error: prompt file not found: <path>` to stderr, exit 1.

---

## List Prompts Command

```
$ yaptap --list-prompts
```

Prints all `.toml` files found in `config/prompts/`, one per line:

```
Available prompts (config/prompts/):
  action-items    Extract action items from a voice note as a bullet list
  email-reply     Rewrite a voice note as a professional email reply
  journal         Clean up a voice journal entry into polished prose
  meeting-notes   Structure a voice brain-dump into clean meeting notes
  slack-message   Rewrite a voice note as a clear, casual Slack message
```

Sorted alphabetically by filename stem. If the directory is empty or does not exist, print `error: prompts directory not found: config/prompts/` to stderr, exit 1.

---

## Error Cases

| Condition | Behaviour |
|---|---|
| `--prompt <name>` and file missing | `error: prompt '<name>' not found in config/prompts/`, exit 1 |
| `--prompt-file <path>` and file missing | `error: prompt file not found: <path>`, exit 1 |
| `--prompt` and `--prompt-file` both given | `error: --prompt and --prompt-file are mutually exclusive`, exit 1 |
| Prompt TOML missing required field | `error: prompt file invalid — missing field '<field>': <path>`, exit 1 |
| Prompt TOML parse error | `error: prompt file is not valid TOML: <path>`, exit 1 |
| `config/prompts/` directory absent | `error: prompts directory not found: config/prompts/`, exit 1 |
