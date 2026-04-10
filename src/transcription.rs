/// Transcription helper — spawns `python3 <script>` as a subprocess, stores
/// the [`Child`] handle in `active_child` so that the caller can kill it on
/// demand (e.g. from a global hot-key), then waits for completion and returns
/// the transcript text.
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// Resolve the path to `src/core/transcribe.py` relative to the running binary.
///
/// Tries three candidates in order, returning the first whose parent directory
/// exists (P6-I007):
///   1. `<binary>/../../../src/core/transcribe.py` — dev build at target/debug/ or
///      target/release/
///   2. `<binary>/../src/core/transcribe.py` — bundled install next to the binary
///   3. `src/core/transcribe.py` — cwd-relative fallback (project root cwd)
fn transcribe_script_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("../../src/core/transcribe.py"),
                dir.join("src/core/transcribe.py"),
            ];
            for candidate in &candidates {
                if candidate.parent().map(|p| p.is_dir()).unwrap_or(false) {
                    return candidate.clone();
                }
            }
        }
    }
    PathBuf::from("src/core/transcribe.py")
}

/// Run Whisper transcription on `wav_path` using the given `model`.
///
/// # Contract
/// - Always passes `--model <model>` to `transcribe.py`.
/// - Stores the spawned [`Child`] in `active_child` (replacing any previous
///   value) so external callers can interrupt it.
/// - Clears `active_child` (sets it to `None`) before returning.
/// - Returns `Err` when the process exits non-zero, with stderr as context.
/// - Returns the raw stdout string (the transcript) on success.
pub fn run_transcription(
    wav_path: &Path,
    model: &str,
    active_child: &Arc<Mutex<Option<Child>>>,
) -> Result<String> {
    // ── Build command ─────────────────────────────────────────────────────────
    let script = transcribe_script_path();
    let mut cmd = Command::new("python3");
    cmd.arg(&script)
        .arg(wav_path)
        .args(["--model", model])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // ── Spawn ─────────────────────────────────────────────────────────────────
    let mut child = cmd
        .spawn()
        .context("while spawning python3 transcribe.py")?;

    // ── Capture stderr handle before storing child ────────────────────────────
    // Take stderr *before* moving child into the mutex so we can read it after
    // the child has been waited on.
    let stderr_handle = child.stderr.take();
    let stdout_handle = child.stdout.take();

    // ── Store child in shared slot ────────────────────────────────────────────
    {
        let mut guard = active_child.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(child);
    }

    // ── Read stdout ───────────────────────────────────────────────────────────
    let mut stdout_str = String::new();
    if let Some(mut stdout) = stdout_handle {
        stdout
            .read_to_string(&mut stdout_str)
            .context("while reading stdout from transcribe.py")?;
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
            c.wait().context("while waiting for transcribe.py")?
        } else {
            // Child was already reaped (e.g. killed externally).
            return Err(anyhow::anyhow!("transcription child was killed"));
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
            "transcription failed — {}",
            stderr_str.trim_end()
        ));
    }

    Ok(stdout_str)
}
