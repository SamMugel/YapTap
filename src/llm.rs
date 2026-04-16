/// LLM helper — spawns `python3 <script>` as a subprocess, writes the
/// transcript to its stdin, collects the entire stdout response into a
/// [`String`], and returns it.
///
/// Unlike the CLI mode in `main.rs` (which streams tokens to the terminal),
/// this function buffers all output so the menu-bar app can decide how to
/// display or paste it.
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// Resolve the path to `src/core/llm.py` relative to the running binary.
///
/// Tries four candidates in order, returning the first whose parent directory
/// exists (P6-I007):
///   1. `Contents/Resources/scripts/llm.py` — bundle layout
///   2. `<binary>/../../src/core/llm.py` — dev build at target/debug/ or
///      target/release/
///   3. `<binary>/src/core/llm.py` — legacy
///   4. `src/core/llm.py` — cwd-relative fallback (project root cwd)
fn llm_script_path() -> PathBuf {
    // Candidate 1: bundle layout (Contents/Resources/scripts/llm.py).
    let resources_candidate = crate::config::resources_dir().join("scripts/llm.py");
    if resources_candidate.parent().map(|p| p.is_dir()).unwrap_or(false) {
        return resources_candidate;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                // Candidate 2: dev build at target/debug/ or target/release/
                dir.join("../../src/core/llm.py"),
                // Candidate 3: legacy
                dir.join("src/core/llm.py"),
            ];
            for candidate in &candidates {
                if candidate.parent().map(|p| p.is_dir()).unwrap_or(false) {
                    return candidate.clone();
                }
            }
        }
    }
    // Candidate 4: cwd fallback (project root)
    PathBuf::from("src/core/llm.py")
}

/// Run the LLM pipeline, collecting all output.
///
/// # Contract
/// - Passes `--prompt-file <prompt_path>` and `--model <model>` to `llm.py`.
/// - Passes `--provider <provider>` to `llm.py`.
/// - If `api_key` is `Some`, injects `MULTIVERSE_IAM_API_KEY` into the subprocess environment.
/// - Writes `transcript` to the child's stdin, then closes stdin.
/// - Stores the spawned [`Child`] in `active_child` (replacing any previous
///   value) so external callers can interrupt it.
/// - Clears `active_child` (sets it to `None`) before returning.
/// - Returns `Err` when the process exits non-zero, with stderr as context.
/// - Returns the complete stdout string (the LLM response) on success.
pub fn run_llm_collect(
    transcript: &str,
    prompt_path: &Path,
    model: &str,
    provider: &str,
    api_key: Option<&str>,
    active_child: &Arc<Mutex<Option<Child>>>,
) -> Result<String> {
    // ── Build command ─────────────────────────────────────────────────────────
    let script = llm_script_path();
    let mut cmd = Command::new(crate::config::python_interpreter());
    cmd.arg(&script)
        .arg("--prompt-file")
        .arg(prompt_path)
        .args(["--model", model])
        .args(["--provider", provider])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", crate::config::brew_augmented_path());

    if let Some(key) = api_key {
        cmd.env("MULTIVERSE_IAM_API_KEY", key);
    }

    // ── Spawn ─────────────────────────────────────────────────────────────────
    tracing::debug!(provider = %provider, has_api_key = api_key.is_some(), model = %model, "spawning llm.py");
    let mut child = cmd.spawn().context("while spawning python3 llm.py")?;

    // ── Write transcript to stdin then close it ───────────────────────────────
    // Take stdin *before* storing child in the mutex so we can write without
    // holding the lock.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(transcript.as_bytes())
            .context("while writing transcript to llm.py stdin")?;
        // `stdin` is dropped here, closing the pipe so llm.py sees EOF.
    }

    // ── Take stdout/stderr handles before storing child ───────────────────────
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    // ── Store child in shared slot ────────────────────────────────────────────
    {
        let mut guard = active_child.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(child);
    }

    // ── Buffer ALL stdout ─────────────────────────────────────────────────────
    let mut stdout_str = String::new();
    if let Some(mut stdout) = stdout_handle {
        stdout
            .read_to_string(&mut stdout_str)
            .context("while reading stdout from llm.py")?;
    }

    // ── Read stderr ───────────────────────────────────────────────────────────
    let mut stderr_str = String::new();
    if let Some(mut stderr) = stderr_handle {
        let _ = stderr.read_to_string(&mut stderr_str);
    }

    // ── Wait for process ──────────────────────────────────────────────────────
    let status = {
        let mut guard = active_child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut c) = *guard {
            c.wait().context("while waiting for llm.py")?
        } else {
            // Child was already reaped (e.g. killed externally).
            return Err(anyhow::anyhow!("LLM child was killed"));
        }
    };

    // ── Clear active child ────────────────────────────────────────────────────
    {
        let mut guard = active_child.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    // ── Check exit code ───────────────────────────────────────────────────────
    if !status.success() {
        return Err(anyhow::anyhow!(
            "LLM generation failed — {}",
            stderr_str.trim_end()
        ));
    }

    Ok(stdout_str)
}
