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

/// Returns the bundle's Resources directory at runtime.
///
/// - **Bundle** (`Contents/MacOS/yaptap`): `../Resources` = `Contents/Resources/` — exists → returned.
/// - **Dev build** (`target/debug/yaptap` from project root): `../Resources` does not exist →
///   `current_dir()` returned.
pub fn resources_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            let candidate = macos_dir.join("../Resources");
            if candidate.exists() {
                return candidate.canonicalize().unwrap_or(candidate);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Returns the Python interpreter path for subprocess calls.
///
/// Returns the absolute venv interpreter path if `~/.config/yaptap/.venv/bin/python` exists;
/// otherwise falls back to `"python3"` (PATH lookup).
pub fn python_interpreter() -> String {
    let venv_python = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/yaptap/.venv/bin/python");
    if venv_python.is_file() {
        venv_python.to_string_lossy().into_owned()
    } else {
        "python3".to_string()
    }
}

/// Returns the prompts directory.
///
/// Candidate order:
/// 1. `resources_dir().join("config/prompts")` — bundle layout
/// 2. `<binary_dir>/config/prompts` — dev build
/// 3. `<cwd>/config/prompts` — cwd fallback
pub fn prompts_dir() -> Option<PathBuf> {
    // Candidate 1: bundle layout (Contents/Resources/config/prompts/).
    let resources_candidate = resources_dir().join("config/prompts");
    if resources_candidate.is_dir() {
        return Some(resources_candidate);
    }
    // Candidate 2: <binary_dir>/config/prompts (dev build).
    let bin_relative = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("config/prompts")));
    if let Some(ref p) = bin_relative {
        if p.is_dir() {
            return bin_relative;
        }
    }
    // Candidate 3: <cwd>/config/prompts (cwd fallback).
    std::env::current_dir()
        .ok()
        .map(|d| d.join("config/prompts"))
}

// ── AppConfig impl ────────────────────────────────────────────────────────────

impl AppConfig {
    /// Load config from disk, creating the file (with defaults) if absent.
    ///
    /// Returns `(AppConfig, Vec<String>)` where the `Vec` carries deferred
    /// human-readable warning messages for TOML parse errors and invalid
    /// hotkey values.  The caller should show these as UI alerts after
    /// `finishLaunching` has been called.
    pub fn load() -> (AppConfig, Vec<String>) {
        let mut warnings: Vec<String> = Vec::new();
        let path = config_path();

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    path = %parent.display(),
                    error = %e,
                    "could not create config directory"
                );
            }
        }

        // Write defaults if the file does not yet exist.
        if !path.exists() {
            let default_cfg = AppConfig::default();
            match toml::to_string_pretty(&default_cfg) {
                Ok(contents) => {
                    if let Err(e) = std::fs::write(&path, &contents) {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "could not write default config"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "could not serialise default config");
                }
            }
            return (default_cfg, warnings);
        }

        // Read and parse.
        let contents = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "could not read config file — using defaults"
                );
                return (AppConfig::default(), warnings);
            }
        };

        let mut cfg: AppConfig = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "config file is invalid TOML — using defaults"
                );
                warnings.push(format!(
                    "Config file is not valid TOML: {} — using defaults.",
                    path.display()
                ));
                return (AppConfig::default(), warnings);
            }
        };

        // Validate hotkey by attempting to parse it.
        if crate::hotkey::parse_hotkey(&cfg.hotkey).is_err() {
            let bad_hotkey = cfg.hotkey.clone();
            tracing::warn!(
                hotkey = %bad_hotkey,
                "invalid hotkey in config — resetting to option+space"
            );
            warnings.push(format!(
                "Unknown hotkey \"{bad_hotkey}\" — using default: option+space."
            ));
            cfg.hotkey = "option+space".to_string();
        }

        // Validate selected_prompt: reset if no matching .toml in prompts_dir.
        if !cfg.selected_prompt.is_empty() {
            let prompt_exists = prompts_dir()
                .map(|d| d.join(format!("{}.toml", cfg.selected_prompt)).exists())
                .unwrap_or(false);
            if !prompt_exists {
                tracing::warn!(
                    prompt = %cfg.selected_prompt,
                    "selected prompt not found — resetting to none"
                );
                cfg.selected_prompt = String::new();
            }
        }

        (cfg, warnings)
    }

    /// Atomically persist the config with `hotkey` set to `new_hotkey`.
    ///
    /// Follows the identical atomic write pattern as `save_prompt`: write to
    /// `.tmp` then rename into place, with EXDEV fallback.
    pub fn save_hotkey(&self, new_hotkey: &str) -> anyhow::Result<()> {
        let mut updated = self.clone();
        updated.hotkey = new_hotkey.to_string();

        let path = config_path();
        let tmp_path = path.with_extension("toml.tmp");

        let contents = match toml::to_string_pretty(&updated) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "could not serialise config");
                return Ok(());
            }
        };

        if let Err(e) = std::fs::write(&tmp_path, &contents) {
            tracing::warn!(
                path = %tmp_path.display(),
                error = %e,
                "could not write config"
            );
            return Ok(());
        }

        if let Err(rename_err) = std::fs::rename(&tmp_path, &path) {
            if rename_err.raw_os_error() == Some(libc_exdev()) {
                if let Err(e) = std::fs::copy(&tmp_path, &path) {
                    tracing::warn!(
                        src = %tmp_path.display(),
                        dst = %path.display(),
                        error = %e,
                        "could not copy config"
                    );
                    let _ = std::fs::remove_file(&tmp_path);
                    return Ok(());
                }
                let _ = std::fs::remove_file(&tmp_path);
            } else {
                tracing::warn!(
                    src = %tmp_path.display(),
                    dst = %path.display(),
                    error = %rename_err,
                    "could not rename config"
                );
                let _ = std::fs::remove_file(&tmp_path);
            }
        }

        Ok(())
    }

    /// Atomically persist the config with `selected_prompt` set to `stem`.
    ///
    /// Writes to a `.tmp` sibling then renames into place.  Falls back to
    /// copy+delete when the rename crosses a filesystem boundary (EXDEV).
    /// Any I/O error is logged via tracing and swallowed so callers need not
    /// handle it.
    pub fn save_prompt(&self, stem: &str) -> anyhow::Result<()> {
        let mut updated = self.clone();
        updated.selected_prompt = stem.to_string();

        let path = config_path();
        let tmp_path = path.with_extension("toml.tmp");

        let contents = match toml::to_string_pretty(&updated) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "could not serialise config");
                return Ok(());
            }
        };

        if let Err(e) = std::fs::write(&tmp_path, &contents) {
            tracing::warn!(
                path = %tmp_path.display(),
                error = %e,
                "could not write config"
            );
            return Ok(());
        }

        // Attempt atomic rename; fall back to copy+delete on EXDEV.
        if let Err(rename_err) = std::fs::rename(&tmp_path, &path) {
            if rename_err.raw_os_error() == Some(libc_exdev()) {
                // Cross-device rename — copy then delete.
                if let Err(e) = std::fs::copy(&tmp_path, &path) {
                    tracing::warn!(
                        src = %tmp_path.display(),
                        dst = %path.display(),
                        error = %e,
                        "could not copy config"
                    );
                    let _ = std::fs::remove_file(&tmp_path);
                    return Ok(());
                }
                let _ = std::fs::remove_file(&tmp_path);
            } else if rename_err.kind() == ErrorKind::NotFound {
                tracing::warn!(
                    src = %tmp_path.display(),
                    dst = %path.display(),
                    error = %rename_err,
                    "could not rename config"
                );
                let _ = std::fs::remove_file(&tmp_path);
            } else {
                tracing::warn!(
                    src = %tmp_path.display(),
                    dst = %path.display(),
                    error = %rename_err,
                    "could not rename config"
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
