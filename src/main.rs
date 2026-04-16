// YapTap — Phase 3 CLI voice transcription + LLM pipeline entry point.
//
// Records microphone audio, writes a temporary WAV file, delegates
// transcription to `python3 src/core/transcribe.py <wav_path>`, and
// optionally pipes the transcript through an LLM via `python3 src/core/llm.py`.
// When invoked with no flags, launches the menu-bar app via `app::run_app()`.

// Required so that objc's `msg_send!` macro (which expands to `sel!`) is
// available in all modules, including `app.rs`.
#[macro_use]
extern crate objc;

mod app;
mod audio;
mod config;
mod hotkey;
mod llm;
mod transcription;

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(name = "yaptap", about = "Voice-to-text CLI")]
struct Args {
    /// Select a named prompt from config/prompts/
    #[arg(long)]
    prompt: Option<String>,

    /// Use a custom prompt TOML file
    #[arg(long)]
    prompt_file: Option<PathBuf>,

    /// Override the Whisper model (default: base)
    #[arg(long)]
    model: Option<String>,

    /// Override the ollama model (default: llama3)
    #[arg(long)]
    llm_model: Option<String>,

    /// List available prompts and exit
    #[arg(long)]
    list_prompts: bool,

    /// Select audio input device by index
    #[arg(long)]
    device: Option<usize>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct PromptToml {
    name: String,
    description: String,
    system: String,
}

// ── Helper functions ──────────────────────────────────────────────────────────

fn validate_prompt_toml(content: &str, path: &Path) {
    match toml::from_str::<toml::Value>(content) {
        Err(_) => {
            eprintln!("error: prompt file is not valid TOML: {}", path.display());
            process::exit(1);
        }
        Ok(val) => {
            for field in &["name", "description", "system"] {
                if val.get(field).is_none() {
                    eprintln!(
                        "error: prompt file invalid — missing field '{field}': {}",
                        path.display()
                    );
                    process::exit(1);
                }
            }
        }
    }
}

fn resolve_prompt_file(args: &Args) -> Option<PathBuf> {
    // Returns None if no prompt flag given; exits process on error
    if let Some(ref name) = args.prompt {
        let dir = match config::prompts_dir() {
            Some(d) if d.is_dir() => d,
            _ => {
                eprintln!("error: prompts directory not found: config/prompts/");
                process::exit(1);
            }
        };
        let path = dir.join(format!("{name}.toml"));
        if !path.exists() {
            eprintln!("error: prompt '{name}' not found in config/prompts/");
            process::exit(1);
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        validate_prompt_toml(&content, &path);
        Some(path)
    } else if let Some(ref file) = args.prompt_file {
        if !file.exists() {
            eprintln!("error: prompt file not found: {}", file.display());
            process::exit(1);
        }
        let content = std::fs::read_to_string(file).unwrap_or_default();
        validate_prompt_toml(&content, file);
        Some(file.clone())
    } else {
        None
    }
}

fn main() -> Result<()> {
    // Initialise structured logging; RUST_LOG controls verbosity.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // ── Parse CLI args ────────────────────────────────────────────────────────
    let args = Args::parse();

    // Mode detection: no flags → app mode
    if args.prompt.is_none()
        && args.prompt_file.is_none()
        && !args.list_prompts
        && args.model.is_none()
        && args.llm_model.is_none()
        && args.device.is_none()
    {
        app::run_app();
        return Ok(());
    }

    // Mutual exclusion check
    if args.prompt.is_some() && args.prompt_file.is_some() {
        eprintln!("error: --prompt and --prompt-file are mutually exclusive");
        process::exit(1);
    }

    // ── Handle --list-prompts (early exit) ────────────────────────────────────
    if args.list_prompts {
        let dir = match config::prompts_dir() {
            Some(d) if d.is_dir() => d,
            _ => {
                eprintln!("error: prompts directory not found: config/prompts/");
                process::exit(1);
            }
        };

        let mut entries: Vec<(String, String)> = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|_| {
            eprintln!("error: prompts directory not found: config/prompts/");
            process::exit(1);
        }) {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                if let Ok(parsed) = toml::from_str::<PromptToml>(&content) {
                    entries.push((stem, parsed.description));
                }
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        println!("Available prompts (config/prompts/):");
        for (stem, desc) in entries {
            println!("  {stem:<16}{desc}");
        }
        process::exit(0);
    }

    // ── Resolve prompt file (if any) ──────────────────────────────────────────
    let prompt_path = resolve_prompt_file(&args);

    // ── 1. SIGINT handler — best-effort WAV cleanup if signal fires late ──────
    // The WAV path isn't known until after stop_and_save(), so we share a
    // slot that gets filled once the file exists.
    let wav_for_sigint: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let llm_active_child: Arc<Mutex<Option<process::Child>>> = Arc::new(Mutex::new(None));
    {
        let wav_for_sigint = Arc::clone(&wav_for_sigint);
        let llm_active_child = Arc::clone(&llm_active_child);
        ctrlc::set_handler(move || {
            if let Ok(mut guard) = llm_active_child.lock() {
                if let Some(ref mut child) = *guard {
                    let _ = child.kill();
                }
            }
            if let Ok(guard) = wav_for_sigint.lock() {
                if let Some(ref p) = *guard {
                    let _ = std::fs::remove_file(p);
                }
            }
            process::exit(130);
        })
        .context("while registering SIGINT handler")?;
    }

    // ── 2. Start non-blocking audio capture ───────────────────────────────────
    let handle = audio::start_recording(args.device, |e| {
        eprintln!("stream error: {e}");
    })?;
    println!("Recording... (press Enter to stop)");

    // ── 3. Elapsed-time counter thread ────────────────────────────────────────
    let stop_flag = Arc::new(AtomicBool::new(false));
    {
        let stop_flag = Arc::clone(&stop_flag);
        thread::spawn(move || {
            let start = Instant::now();
            let mut last_printed = 0u64;
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                let elapsed = start.elapsed().as_secs();
                if elapsed != last_printed {
                    last_printed = elapsed;
                    let mins = elapsed / 60;
                    let secs = elapsed % 60;
                    print!("\r\u{258e} {mins}:{secs:02}");
                    let _ = io::stdout().flush();
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    // ── 4. Block until user presses Enter ────────────────────────────────────
    {
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }

    // ── 5. Stop capture and encode WAV ───────────────────────────────────────
    stop_flag.store(true, Ordering::SeqCst);
    println!();

    let wav_path = handle
        .stop_and_save()
        .context("while stopping audio and encoding WAV")?;
    // Register the now-known path for SIGINT cleanup.
    *wav_for_sigint.lock().unwrap() = Some(wav_path.clone());

    // ── 6. Validate python3 ───────────────────────────────────────────────────
    let py_check = process::Command::new("python3")
        .arg("--version")
        .output();
    match py_check {
        Ok(out) if out.status.success() => {}
        _ => {
            eprintln!("error: python3 not found");
            let _ = std::fs::remove_file(&wav_path);
            process::exit(1);
        }
    }

    // ── 7. Validate ffmpeg ────────────────────────────────────────────────────
    let ff_check = process::Command::new("ffmpeg")
        .arg("-version")
        .output();
    match ff_check {
        Ok(out) if out.status.success() => {}
        _ => {
            eprintln!("error: ffmpeg not found");
            let _ = std::fs::remove_file(&wav_path);
            process::exit(1);
        }
    }

    tracing::debug!(path = ?wav_path, "WAV ready for transcription");

    // ── 8. Transcribe ─────────────────────────────────────────────────────────
    println!("Transcribing...");

    let whisper_model = args.model.as_deref().unwrap_or("base");
    let cli_active_child: Arc<Mutex<Option<process::Child>>> = Arc::new(Mutex::new(None));
    let transcript = match transcription::run_transcription(&wav_path, whisper_model, &cli_active_child) {
        Ok(t) => t,
        Err(e) => {
            let _ = std::fs::remove_file(&wav_path);
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    // Remove temp file regardless of outcome.
    let _ = std::fs::remove_file(&wav_path);

    // ── 9. LLM pipeline (Phase 2) or print transcript (Phase 1) ─────────────
    if let Some(ref ppath) = prompt_path {
        println!("Thinking...");

        let llm_model = args.llm_model.as_deref().unwrap_or("llama3");

        let mut llm_child = process::Command::new("python3")
            .arg("src/core/llm.py")
            .arg("--prompt-file")
            .arg(ppath)
            .args(["--model", llm_model])
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .spawn()
            .context("while spawning llm.py")?;

        // Write transcript to stdin
        if let Some(mut stdin) = llm_child.stdin.take() {
            let _ = stdin.write_all(transcript.as_bytes());
            // stdin dropped here, closing the pipe
        }

        // Collect stderr in a background thread
        let stderr_handle = llm_child.stderr.take().map(|stderr| {
            thread::spawn(move || {
                use std::io::Read;
                let mut s = String::new();
                let mut r = stderr;
                let _ = r.read_to_string(&mut s);
                s
            })
        });

        // Take stdout before storing child so we can stream independently
        let llm_stdout = llm_child.stdout.take();

        // Store child in shared slot so the SIGINT handler can kill it
        {
            let mut guard = llm_active_child.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(llm_child);
        }

        // Stream stdout to terminal
        if let Some(mut stdout) = llm_stdout {
            use std::io::Read;
            let mut buf = [0u8; 256];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = io::stdout().write_all(&buf[..n]);
                        let _ = io::stdout().flush();
                    }
                    Err(_) => break,
                }
            }
        }

        let status = {
            let mut guard = llm_active_child.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut c) = *guard {
                c.wait().context("while waiting for llm.py")?
            } else {
                return Err(anyhow::anyhow!("LLM child was killed"));
            }
        };

        // Clear the shared slot
        {
            let mut guard = llm_active_child.lock().unwrap_or_else(|e| e.into_inner());
            *guard = None;
        }

        let stderr_output = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        if !status.success() {
            eprintln!(
                "error: LLM generation failed — {}",
                stderr_output.trim_end()
            );
            process::exit(1);
        }

        return Ok(());
    }

    // Phase 1 behavior: print transcript to stdout
    print!("{transcript}");
    if !transcript.ends_with('\n') {
        println!();
    }

    Ok(())
}
