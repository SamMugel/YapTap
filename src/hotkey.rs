use anyhow::anyhow;

/// A parsed hotkey consisting of zero or more modifier keys and one main key.
pub struct ParsedHotkey {
    pub modifiers: Vec<rdev::Key>,
    pub key: rdev::Key,
}

/// Parse a hotkey string of the form `<modifier>[+<modifier>...]+<key>`.
///
/// Valid modifiers (case-sensitive, all lowercase):
///   `option` → `rdev::Key::Alt`
///   `cmd`    → `rdev::Key::MetaLeft`
///   `ctrl`   → `rdev::Key::ControlLeft`
///   `shift`  → `rdev::Key::ShiftLeft`
///
/// Valid main keys: any single printable char (`a`-`z`, `0`-`9`), or the
/// named keys `space`, `tab`, `return`, `escape`, `delete`, `left`, `right`,
/// `up`, `down`, `f1`-`f20`.
///
/// Returns `Err` if any token is unrecognised.
pub fn parse_hotkey(s: &str) -> anyhow::Result<ParsedHotkey> {
    let tokens: Vec<&str> = s.split('+').collect();

    if tokens.is_empty() {
        return Err(anyhow!("hotkey string is empty"));
    }

    // Last token is the main key; everything before it is a modifier.
    let (modifier_tokens, key_token) = tokens.split_at(tokens.len() - 1);
    let key_token = key_token[0];

    let mut modifiers = Vec::with_capacity(modifier_tokens.len());
    for &tok in modifier_tokens {
        let modifier = parse_modifier(tok)
            .ok_or_else(|| anyhow!("unrecognised modifier: {tok:?}"))?;
        modifiers.push(modifier);
    }

    let key = parse_key(key_token)
        .ok_or_else(|| anyhow!("unrecognised key: {key_token:?}"))?;

    Ok(ParsedHotkey { modifiers, key })
}

fn parse_modifier(tok: &str) -> Option<rdev::Key> {
    match tok {
        "option" => Some(rdev::Key::Alt),
        "cmd"    => Some(rdev::Key::MetaLeft),
        "ctrl"   => Some(rdev::Key::ControlLeft),
        "shift"  => Some(rdev::Key::ShiftLeft),
        _ => None,
    }
}

fn parse_key(tok: &str) -> Option<rdev::Key> {
    // Named special keys
    match tok {
        "space"  => return Some(rdev::Key::Space),
        "tab"    => return Some(rdev::Key::Tab),
        "return" => return Some(rdev::Key::Return),
        "escape" => return Some(rdev::Key::Escape),
        "delete" => return Some(rdev::Key::Backspace),
        "left"   => return Some(rdev::Key::LeftArrow),
        "right"  => return Some(rdev::Key::RightArrow),
        "up"     => return Some(rdev::Key::UpArrow),
        "down"   => return Some(rdev::Key::DownArrow),
        "f1"     => return Some(rdev::Key::F1),
        "f2"     => return Some(rdev::Key::F2),
        "f3"     => return Some(rdev::Key::F3),
        "f4"     => return Some(rdev::Key::F4),
        "f5"     => return Some(rdev::Key::F5),
        "f6"     => return Some(rdev::Key::F6),
        "f7"     => return Some(rdev::Key::F7),
        "f8"     => return Some(rdev::Key::F8),
        "f9"     => return Some(rdev::Key::F9),
        "f10"    => return Some(rdev::Key::F10),
        "f11"    => return Some(rdev::Key::F11),
        "f12"    => return Some(rdev::Key::F12),
        // rdev 0.5.3 does not have F13-F20 named variants; they map to
        // Unknown(u32) with macOS CGKeyCode values (105,107,113,106,64,79,80,90).
        // When rdev adds F13-F20 variants, update these arms.
        "f13"    => return Some(rdev::Key::Unknown(105)),
        "f14"    => return Some(rdev::Key::Unknown(107)),
        "f15"    => return Some(rdev::Key::Unknown(113)),
        "f16"    => return Some(rdev::Key::Unknown(106)),
        "f17"    => return Some(rdev::Key::Unknown(64)),
        "f18"    => return Some(rdev::Key::Unknown(79)),
        "f19"    => return Some(rdev::Key::Unknown(80)),
        "f20"    => return Some(rdev::Key::Unknown(90)),
        _ => {}
    }

    // Single-character keys
    if tok.len() == 1 {
        let ch = tok.chars().next().unwrap();
        return match ch {
            'a' => Some(rdev::Key::KeyA),
            'b' => Some(rdev::Key::KeyB),
            'c' => Some(rdev::Key::KeyC),
            'd' => Some(rdev::Key::KeyD),
            'e' => Some(rdev::Key::KeyE),
            'f' => Some(rdev::Key::KeyF),
            'g' => Some(rdev::Key::KeyG),
            'h' => Some(rdev::Key::KeyH),
            'i' => Some(rdev::Key::KeyI),
            'j' => Some(rdev::Key::KeyJ),
            'k' => Some(rdev::Key::KeyK),
            'l' => Some(rdev::Key::KeyL),
            'm' => Some(rdev::Key::KeyM),
            'n' => Some(rdev::Key::KeyN),
            'o' => Some(rdev::Key::KeyO),
            'p' => Some(rdev::Key::KeyP),
            'q' => Some(rdev::Key::KeyQ),
            'r' => Some(rdev::Key::KeyR),
            's' => Some(rdev::Key::KeyS),
            't' => Some(rdev::Key::KeyT),
            'u' => Some(rdev::Key::KeyU),
            'v' => Some(rdev::Key::KeyV),
            'w' => Some(rdev::Key::KeyW),
            'x' => Some(rdev::Key::KeyX),
            'y' => Some(rdev::Key::KeyY),
            'z' => Some(rdev::Key::KeyZ),
            '0' => Some(rdev::Key::Num0),
            '1' => Some(rdev::Key::Num1),
            '2' => Some(rdev::Key::Num2),
            '3' => Some(rdev::Key::Num3),
            '4' => Some(rdev::Key::Num4),
            '5' => Some(rdev::Key::Num5),
            '6' => Some(rdev::Key::Num6),
            '7' => Some(rdev::Key::Num7),
            '8' => Some(rdev::Key::Num8),
            '9' => Some(rdev::Key::Num9),
            _ => None,
        };
    }

    None
}

// ── Accessibility permission check ──────────────────────────────────────────

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Returns `true` if the current process has been granted Accessibility
/// permission (required for global key capture via `rdev`).
pub fn ax_is_process_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

// ── Input Monitoring permission check (macOS 14+) ───────────────────────────
//
// On macOS 14 (Sonoma) and later, CGEventTap requires BOTH Accessibility AND
// Input Monitoring (Privacy & Security → Input Monitoring) to receive keyboard
// events system-wide.  If Input Monitoring is absent, CGEventTapCreate
// succeeds and rdev::listen() returns no error, but no key events are
// delivered — the hotkey silently does nothing (P4-I002).

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
}

/// Returns `true` if the process has Input Monitoring (Listen Event) access.
/// On macOS 13 and earlier this always returns `true` (not enforced).
/// On macOS 14+ the user must grant this in System Settings.
pub fn input_monitoring_trusted() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_and_key() {
        let hk = parse_hotkey("option+cmd+r").unwrap();
        assert_eq!(hk.modifiers, vec![rdev::Key::Alt, rdev::Key::MetaLeft]);
        assert_eq!(hk.key, rdev::Key::KeyR);
    }

    #[test]
    fn test_no_modifier() {
        let hk = parse_hotkey("space").unwrap();
        assert!(hk.modifiers.is_empty());
        assert_eq!(hk.key, rdev::Key::Space);
    }

    #[test]
    fn test_single_char_key() {
        let hk = parse_hotkey("ctrl+shift+a").unwrap();
        assert_eq!(hk.modifiers, vec![rdev::Key::ControlLeft, rdev::Key::ShiftLeft]);
        assert_eq!(hk.key, rdev::Key::KeyA);
    }

    #[test]
    fn test_digit_key() {
        let hk = parse_hotkey("cmd+1").unwrap();
        assert_eq!(hk.key, rdev::Key::Num1);
    }

    #[test]
    fn test_function_key() {
        let hk = parse_hotkey("f5").unwrap();
        assert_eq!(hk.key, rdev::Key::F5);
    }

    #[test]
    fn test_unknown_modifier_errors() {
        assert!(parse_hotkey("win+r").is_err());
    }

    #[test]
    fn test_f13_is_valid() {
        // rdev 0.5.3 lacks F13 named variant; we use Unknown(105) (macOS CGKeyCode).
        let hk = parse_hotkey("cmd+f13").unwrap();
        assert_eq!(hk.key, rdev::Key::Unknown(105));
    }

    #[test]
    fn test_f20_is_valid() {
        // rdev 0.5.3 lacks F20 named variant; we use Unknown(90) (macOS CGKeyCode).
        let hk = parse_hotkey("f20").unwrap();
        assert_eq!(hk.key, rdev::Key::Unknown(90));
    }

    #[test]
    fn test_unknown_key_errors() {
        assert!(parse_hotkey("cmd+f21").is_err());
    }

    #[test]
    fn test_arrow_keys() {
        let hk = parse_hotkey("option+left").unwrap();
        assert_eq!(hk.key, rdev::Key::LeftArrow);
    }
}
