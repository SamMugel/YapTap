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
/// In app mode the cwd is often not the project root, so we anchor to the
/// binary's parent directory and walk up to find the script.  Falls back to
/// the cwd-relative path for development builds.
fn llm_script_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("../../src/core/llm.py")))
        .unwrap_or_else(|| PathBuf::from("src/core/llm.py"))
}

/// Run the LLM pipeline, collecting all output.
///
/// # Contract
/// - Passes `--prompt-file <prompt_path>` and `--model <model>` to `llm.py`.
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
    active_child: &Arc<Mutex<Option<Child>>>,
) -> Result<String> {
    // ── Build command ─────────────────────────────────────────────────────────
    let script = llm_script_path();
    let mut cmd = Command::new("python3");
    cmd.arg(&script)
        .arg("--prompt-file")
        .arg(prompt_path)
        .args(["--model", model])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // ── Spawn ─────────────────────────────────────────────────────────────────
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
