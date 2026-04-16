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
# hotkey: change via in-app dialog (takes effect immediately), or edit here and restart
# whisper_model, llm_model, llm_provider: edit here and restart to apply

hotkey = "option+space"    # global hotkey to start/stop recording
selected_prompt = ""       # stem of selected prompt file; empty = No Prompt
whisper_model = "base"     # Whisper model for transcription
llm_model = "llama3"       # model name for the active LLM provider (see llm_provider)
llm_provider = "ollama"    # LLM provider: "ollama" (local) or "compactifai" (cloud)
```

---

## Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `hotkey` | string | `"option+space"` | Global hotkey used to start and stop recording |
| `selected_prompt` | string | `""` | Filename stem of the active prompt (e.g. `"email-reply"`), or `""` for No Prompt |
| `whisper_model` | string | `"base"` | Whisper model name passed to `transcribe.py` |
| `llm_model` | string | `"llama3"` | Model name passed to `llm.py` as `--model`; interpreted relative to `llm_provider` |
| `llm_provider` | string | `"ollama"` | LLM provider: `"ollama"` (local, default) or `"compactifai"` (cloud). When switching to `"compactifai"` also update `llm_model` to a valid CompactifAI model name (e.g. `"cai-llama-3-1-8b-slim"`). |

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
| Write (`whisper_model`, `llm_model`, `llm_provider`) | Never — user edits the file directly | — |

**Atomic write:** the app writes to `~/.config/yaptap/config.toml.tmp`, then renames it to `config.toml`. This prevents a corrupted file on crash during write.

**Restart required for:** `whisper_model`, `llm_model`, `llm_provider`. After editing these fields in the file, quit and re-launch YapTap. `hotkey` changes made via the in-app dialog take effect immediately; changes made by hand in the file require a restart. `selected_prompt` changes made by hand are overwritten the next time the user picks from the menu.

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
| Unknown `llm_provider` value | Alert: *"Unknown llm_provider '<value>' — using default: ollama."* |
| `llm_provider = "compactifai"` and API key not configured | CLI: prompts for key on stdin, appends to `~/.zshrc`. Menu bar: shows macOS dialog, stores in Keychain. See [llm.md](llm.md) § API Key Management. |
