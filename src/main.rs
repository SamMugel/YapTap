/// YapTap — Phase 1 CLI voice transcription entry point.
///
/// Records microphone audio, writes a temporary WAV file, then delegates
/// transcription to `python3 src/core/transcribe.py <wav_path>`.
use std::{
    io::{self, Write},
    path::PathBuf,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};

fn main() -> Result<()> {
    // Initialise structured logging; RUST_LOG controls verbosity.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // ── 1. Resolve default input device ──────────────────────────────────────
    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            eprintln!("error: no input device found");
            process::exit(1);
        }
    };

    // ── 2. Negotiate input config: prefer 16 kHz mono i16 ────────────────────
    let supported_configs = device
        .supported_input_configs()
        .context("while querying supported input configs")?;

    // Collect all configs that support mono (channels == 1), or fall back to
    // any channel count if none exist.  Pick the one whose sample-rate range
    // is closest to 16 000 Hz.
    const TARGET_HZ: u32 = 16_000;

    let mut best: Option<cpal::SupportedStreamConfig> = None;
    let mut best_dist = u32::MAX;

    for range in supported_configs {
        let min = range.min_sample_rate().0;
        let max = range.max_sample_rate().0;

        // Clamp target into [min, max] and compute distance.
        let clamped = TARGET_HZ.clamp(min, max);
        let dist = clamped.abs_diff(TARGET_HZ);

        // Prefer mono; deprioritise multi-channel configs when a mono option
        // already exists.
        let channels = range.channels();
        let is_mono = channels == 1;
        let incumbent_is_mono = best
            .as_ref()
            .map(|c| c.channels() == 1)
            .unwrap_or(false);
        if incumbent_is_mono && !is_mono {
            continue;
        }

        if dist < best_dist || (!incumbent_is_mono && is_mono) {
            best_dist = dist;
            best = Some(range.with_sample_rate(cpal::SampleRate(clamped)));
        }
    }

    let stream_config = match best {
        Some(c) => c,
        None => {
            eprintln!("error: no supported input config found");
            process::exit(1);
        }
    };

    let sample_format = stream_config.sample_format();
    let actual_channels = stream_config.channels() as usize;
    let actual_rate = stream_config.sample_rate().0;
    tracing::debug!(
        sample_format = ?sample_format,
        channels = actual_channels,
        sample_rate = actual_rate,
        "negotiated input config"
    );

    // ── 3. Temp WAV path ──────────────────────────────────────────────────────
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wav_path: PathBuf = std::env::temp_dir().join(format!("yaptap_{timestamp}.wav"));
    let wav_path_for_sigint = wav_path.clone();

    // ── 4. SIGINT handler ─────────────────────────────────────────────────────
    let sigint_fired = Arc::new(AtomicBool::new(false));
    let sigint_fired_clone = Arc::clone(&sigint_fired);

    ctrlc::set_handler(move || {
        sigint_fired_clone.store(true, Ordering::SeqCst);
        // Best-effort removal of the temp file.
        let _ = std::fs::remove_file(&wav_path_for_sigint);
        process::exit(130);
    })
    .context("while registering SIGINT handler")?;

    // ── 5. Shared PCM sample buffer ───────────────────────────────────────────
    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let samples_writer = Arc::clone(&samples);

    // ── 6. Build cpal stream ──────────────────────────────────────────────────
    let config: cpal::StreamConfig = stream_config.config();

    /// Convert an f32 sample in [-1.0, 1.0] to i16.
    fn f32_to_i16(s: f32) -> i16 {
        (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16
    }

    /// Down-mix multi-channel interleaved samples to mono i16 and push.
    fn push_mono(buf: &Arc<Mutex<Vec<i16>>>, mono: i16) {
        if let Ok(mut guard) = buf.lock() {
            guard.push(mono);
        }
    }

    let err_fn = |e: cpal::StreamError| {
        tracing::error!("stream error: {}", e);
    };

    let stream = match sample_format {
        SampleFormat::I16 => {
            let buf = Arc::clone(&samples_writer);
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    // Down-mix to mono by averaging channels.
                    for frame in data.chunks(actual_channels) {
                        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                        let mono = (sum / actual_channels as i32).clamp(
                            i16::MIN as i32,
                            i16::MAX as i32,
                        ) as i16;
                        push_mono(&buf, mono);
                    }
                },
                err_fn,
                None,
            )
        }
        SampleFormat::F32 => {
            let buf = Arc::clone(&samples_writer);
            device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    for frame in data.chunks(actual_channels) {
                        let sum: f32 = frame.iter().sum();
                        let avg = sum / actual_channels as f32;
                        push_mono(&buf, f32_to_i16(avg));
                    }
                },
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            let buf = Arc::clone(&samples_writer);
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    for frame in data.chunks(actual_channels) {
                        let sum: i32 = frame
                            .iter()
                            .map(|&s| s as i32 - 32_768)
                            .sum();
                        let mono = (sum / actual_channels as i32).clamp(
                            i16::MIN as i32,
                            i16::MAX as i32,
                        ) as i16;
                        push_mono(&buf, mono);
                    }
                },
                err_fn,
                None,
            )
        }
        other => {
            eprintln!("error: unsupported sample format {other:?}");
            process::exit(1);
        }
    }
    .context("while building input stream")?;

    // ── 7. Start capture ──────────────────────────────────────────────────────
    stream.play().context("while starting input stream")?;
    println!("Recording... (press Enter to stop)");

    // ── 8. Elapsed-time counter thread ────────────────────────────────────────
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_timer = Arc::clone(&stop_flag);

    let timer_handle = thread::spawn(move || {
        let start = Instant::now();
        let mut last_printed = 0u64;
        loop {
            if stop_flag_timer.load(Ordering::Relaxed) {
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

    // ── 9. Stdin thread — blocks until user presses Enter ─────────────────────
    let (tx, rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
        let _ = tx.send(());
    });

    // Block main thread until Enter or SIGINT.
    rx.recv().ok();

    // ── 10. Stop capture ──────────────────────────────────────────────────────
    stop_flag.store(true, Ordering::SeqCst);
    drop(stream); // closes the cpal stream
    timer_handle.join().ok();

    // Newline after the last \r▐ X:XX line.
    println!();

    // ── 11. Validate python3 ──────────────────────────────────────────────────
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

    // ── 12. Validate ffmpeg ───────────────────────────────────────────────────
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

    // ── 13. Encode captured samples to WAV ───────────────────────────────────
    // Always write a 16 kHz mono 16-bit PCM WAV regardless of what the device
    // actually captured; resample naively if the device returned a different
    // rate (for Phase 1 simplicity, nearest-neighbour decimation/duplication).
    let pcm_samples = {
        let guard = samples.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    };

    // Resample to 16 kHz if needed.
    let resampled: Vec<i16> = if actual_rate == TARGET_HZ {
        pcm_samples
    } else {
        // Nearest-neighbour: map each output sample index to an input index.
        let out_len =
            (pcm_samples.len() as f64 * TARGET_HZ as f64 / actual_rate as f64) as usize;
        (0..out_len)
            .map(|i| {
                let src_idx =
                    (i as f64 * actual_rate as f64 / TARGET_HZ as f64) as usize;
                pcm_samples[src_idx.min(pcm_samples.len() - 1)]
            })
            .collect()
    };

    let wav_spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_HZ,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    {
        let mut writer = hound::WavWriter::create(&wav_path, wav_spec)
            .context("while creating WAV writer")?;
        for &s in &resampled {
            writer.write_sample(s).context("while writing WAV sample")?;
        }
        writer.finalize().context("while finalizing WAV file")?;
    }

    tracing::debug!(path = ?wav_path, samples = resampled.len(), "WAV written");

    // ── 14. Transcribe ────────────────────────────────────────────────────────
    println!("Transcribing...");

    let output = process::Command::new("python3")
        .arg("src/core/transcribe.py")
        .arg(&wav_path)
        .output()
        .context("while spawning python3 transcribe.py")?;

    // Remove temp file regardless of outcome.
    let _ = std::fs::remove_file(&wav_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("error: transcription failed — {}", stderr.trim_end());
        process::exit(1);
    }

    let transcript = String::from_utf8_lossy(&output.stdout);
    print!("{transcript}");
    if !transcript.ends_with('\n') {
        println!();
    }

    Ok(())
}
