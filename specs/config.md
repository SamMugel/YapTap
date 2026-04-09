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
# Edit this file and restart YapTap to apply changes to the hotkey.

hotkey = "option+space"    # global hotkey to start/stop recording
selected_prompt = ""       # stem of selected prompt file; empty = No Prompt
```

---

## Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `hotkey` | string | `"option+space"` | Global hotkey used to start and stop recording |
| `selected_prompt` | string | `""` | Filename stem of the active prompt (e.g. `"email-reply"`), or `""` for No Prompt |

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
| Write (`hotkey`) | Never — user edits the file directly | — |

**Atomic write:** the app writes to `~/.config/yaptap/config.toml.tmp`, then renames it to `config.toml`. This prevents a corrupted file on crash during write.

**Hotkey changes require a restart.** After editing `hotkey` in the config file, quit and re-launch YapTap.

---

## Error Cases

| Condition | Behaviour |
|---|---|
| File absent on launch | Created with defaults; no error shown |
| `~/.config/yaptap/` absent | Directory and file created with defaults |
| TOML parse error | Alert: *"Config file is not valid TOML: ~/.config/yaptap/config.toml — using defaults."* |
| Unknown or malformed `hotkey` value | Alert: *"Unknown hotkey '<value>' — using default: option+space."* |
| `selected_prompt` stem not found in `config/prompts/` | Falls back to No Prompt silently; corrects the in-memory selection but does not rewrite the file |
| Atomic write failure | Log warning to stderr; continue with in-memory state |
