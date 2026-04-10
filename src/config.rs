/// YapTap — Phase 3 configuration management.
///
/// Owns `AppConfig` (persisted to `~/.config/yaptap/config.toml`) and the
/// `prompts_dir()` helper that was previously inlined in `main.rs`.
use std::{
    io::ErrorKind,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

// ── AppConfig ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub hotkey: String,
    pub selected_prompt: String,
    pub whisper_model: String,
    pub llm_model: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "option+space".to_string(),
            selected_prompt: String::new(),
            whisper_model: "base".to_string(),
            llm_model: "llama3".to_string(),
        }
    }
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Returns `~/.config/yaptap/config.toml`.
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/yaptap/config.toml")
}

/// Returns the prompts directory.
///
/// Prefers `<binary_dir>/config/prompts` (installed layout); falls back to
/// `<cwd>/config/prompts` for development builds.
pub fn prompts_dir() -> Option<PathBuf> {
    let bin_relative = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("config/prompts")));
    if let Some(ref p) = bin_relative {
        if p.is_dir() {
            return bin_relative;
        }
    }
    std::env::current_dir()
        .ok()
        .map(|d| d.join("config/prompts"))
}

// ── AppConfig impl ────────────────────────────────────────────────────────────

impl AppConfig {
    /// Load config from disk, creating the file (with defaults) if absent.
    ///
    /// On any parse / validation error a warning is printed to stderr and the
    /// default value is used — the caller should show a UI alert if desired.
    pub fn load() -> AppConfig {
        let path = config_path();

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "warning: could not create config directory {}: {}",
                    parent.display(),
                    e
                );
            }
        }

        // Write defaults if the file does not yet exist.
        if !path.exists() {
            let default_cfg = AppConfig::default();
            match toml::to_string_pretty(&default_cfg) {
                Ok(contents) => {
                    if let Err(e) = std::fs::write(&path, &contents) {
                        eprintln!(
                            "warning: could not write default config to {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
                Err(e) => {
                    eprintln!("warning: could not serialise default config: {e}");
                }
            }
            return default_cfg;
        }

        // Read and parse.
        let contents = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "warning: could not read config file {}: {} — using defaults",
                    path.display(),
                    e
                );
                return AppConfig::default();
            }
        };

        let mut cfg: AppConfig = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "warning: config file {} is invalid TOML ({}) — using defaults",
                    path.display(),
                    e
                );
                return AppConfig::default();
            }
        };

        // Validate hotkey by attempting to parse it.
        if crate::hotkey::parse_hotkey(&cfg.hotkey).is_err() {
            eprintln!(
                "warning: invalid hotkey {:?} in config — resetting to \"option+space\"",
                cfg.hotkey
            );
            cfg.hotkey = "option+space".to_string();
        }

        // Validate selected_prompt: reset if no matching .toml in prompts_dir.
        if !cfg.selected_prompt.is_empty() {
            let prompt_exists = prompts_dir()
                .map(|d| d.join(format!("{}.toml", cfg.selected_prompt)).exists())
                .unwrap_or(false);
            if !prompt_exists {
                eprintln!(
                    "warning: selected prompt {:?} not found — resetting to none",
                    cfg.selected_prompt
                );
                cfg.selected_prompt = String::new();
            }
        }

        cfg
    }

    /// Atomically persist the config with `selected_prompt` set to `stem`.
    ///
    /// Writes to a `.tmp` sibling then renames into place.  Falls back to
    /// copy+delete when the rename crosses a filesystem boundary (EXDEV).
    /// Any I/O error is logged to stderr and swallowed so callers need not
    /// handle it.
    pub fn save_prompt(&self, stem: &str) -> anyhow::Result<()> {
        let mut updated = self.clone();
        updated.selected_prompt = stem.to_string();

        let path = config_path();
        let tmp_path = path.with_extension("toml.tmp");

        let contents = match toml::to_string_pretty(&updated) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: could not serialise config: {e}");
                return Ok(());
            }
        };

        if let Err(e) = std::fs::write(&tmp_path, &contents) {
            eprintln!(
                "warning: could not write config to {}: {}",
                tmp_path.display(),
                e
            );
            return Ok(());
        }

        // Attempt atomic rename; fall back to copy+delete on EXDEV.
        if let Err(rename_err) = std::fs::rename(&tmp_path, &path) {
            if rename_err.raw_os_error() == Some(libc_exdev()) {
                // Cross-device rename — copy then delete.
                if let Err(e) = std::fs::copy(&tmp_path, &path) {
                    eprintln!(
                        "warning: could not copy config {} -> {}: {}",
                        tmp_path.display(),
                        path.display(),
                        e
                    );
                    let _ = std::fs::remove_file(&tmp_path);
                    return Ok(());
                }
                let _ = std::fs::remove_file(&tmp_path);
            } else if rename_err.kind() == ErrorKind::NotFound {
                // Destination directory may have disappeared; already warned above.
                eprintln!(
                    "warning: could not rename config {} -> {}: {}",
                    tmp_path.display(),
                    path.display(),
                    rename_err
                );
                let _ = std::fs::remove_file(&tmp_path);
            } else {
                eprintln!(
                    "warning: could not rename config {} -> {}: {}",
                    tmp_path.display(),
                    path.display(),
                    rename_err
                );
                let _ = std::fs::remove_file(&tmp_path);
            }
        }

        Ok(())
    }
}

// ── EXDEV constant (cross-device link error) ──────────────────────────────────

#[cfg(unix)]
fn libc_exdev() -> i32 {
    // POSIX error number for cross-device link.
    18 // EXDEV is 18 on macOS and Linux
}

#[cfg(not(unix))]
fn libc_exdev() -> i32 {
    // Non-Unix platforms: use an impossible sentinel so the branch is never taken.
    -1
}
