# Configuration File (Phase 3)

## Location

```
~/.config/yaptap/config.toml
```

Created automatically on first launch with default values. The directory `~/.config/yaptap/` is created if it does not exist.

---

## Format

```toml
# YapTap configuration
# Edit this file and restart YapTap to apply changes to hotkey or model fields.

hotkey = "option+space"    # global hotkey to start/stop recording
selected_prompt = ""       # stem of selected prompt file; empty = No Prompt
whisper_model = "base"     # Whisper model for transcription
llm_model = "llama3"       # Ollama model for LLM inference
```

---

## Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `hotkey` | string | `"option+space"` | Global hotkey used to start and stop recording |
| `selected_prompt` | string | `""` | Filename stem of the active prompt (e.g. `"email-reply"`), or `""` for No Prompt |
| `whisper_model` | string | `"base"` | Whisper model name passed to `transcribe.py` |
| `llm_model` | string | `"llama3"` | Ollama model name passed to `llm.py` as `--model` |

---

## Hotkey Format

```
<modifier>[+<modifier>...]+<key>
```

All tokens are lowercase. The key must be the last token.

**Valid modifiers:**

| Token | Key |
|---|---|
| `option` | ⌥ Option |
| `cmd` | ⌘ Command |
| `ctrl` | ⌃ Control |
| `shift` | ⇧ Shift |

**Valid keys:**

| Token | Key |
|---|---|
| Any printable character | e.g. `a`, `1`, `,`, `/` |
| `space` | Space bar |
| `tab` | Tab |
| `return` | Return / Enter |
| `escape` | Escape |
| `delete` | Delete (backspace) |
| `f1`–`f20` | Function keys |
| `left`, `right`, `up`, `down` | Arrow keys |

### Examples

| Config value | Keys |
|---|---|
| `"option+space"` | ⌥Space (default) |
| `"cmd+shift+y"` | ⌘⇧Y |
| `"ctrl+option+space"` | ⌃⌥Space |
| `"option+f1"` | ⌥F1 |

---

## Read / Write Behaviour

| Operation | When | Who |
|---|---|---|
| Read | On launch | Rust app |
| Write (`selected_prompt`) | When user picks a prompt from the menu | Rust app (atomic write) |
| Write (`hotkey`) | When user changes hotkey via in-app dialog | Rust app (atomic write) |
| Write (`whisper_model`, `llm_model`) | Never — user edits the file directly | — |

**Atomic write:** the app writes to `~/.config/yaptap/config.toml.tmp`, then renames it to `config.toml`. This prevents a corrupted file on crash during write.

**Restart required for:** `whisper_model`, `llm_model`. After editing these fields in the file, quit and re-launch YapTap. `hotkey` changes made via the in-app dialog take effect immediately; changes made by hand in the file require a restart. `selected_prompt` changes made by hand are overwritten the next time the user picks from the menu.

---

## Error Cases

| Condition | Behaviour |
|---|---|
| File absent on launch | Created with defaults; no error shown |
| `~/.config/yaptap/` absent | Directory and file created with defaults |
| TOML parse error | Alert: *"Config file is not valid TOML: ~/.config/yaptap/config.toml — using defaults."* |
| Unknown or malformed `hotkey` value | Alert: *"Unknown hotkey '<value>' — using default: option+space."* |
| `selected_prompt` stem not found in `config/prompts/` | Falls back to No Prompt silently; corrects the in-memory selection but does not rewrite the file — the stale value persists on disk until the user makes an explicit selection from the menu, at which point the atomic write overwrites it |
| Atomic write failure | Log warning to stderr; continue with in-memory state |
