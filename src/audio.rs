/// audio.rs — Phase 3 audio capture module (P3-T003, P3-T004, P3-T005)
///
/// Provides `AudioHandle` and the two public functions:
///   - `start_recording(device_index)` — opens a cpal input stream and returns
///     immediately with a handle to the live capture.
///   - `AudioHandle::stop_and_save(self)` — drops the stream, resamples to
///     16 kHz if needed, encodes a WAV via hound, and returns the temp path.
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};

// ── Constants ─────────────────────────────────────────────────────────────────

const TARGET_HZ: u32 = 16_000;

// ── Public types ──────────────────────────────────────────────────────────────

/// Live recording handle returned by [`start_recording`].
///
/// Drop (or call [`stop_and_save`]) to end capture.  The [`cpal::Stream`] is
/// held here; dropping it signals the driver to stop.
pub struct AudioHandle {
    /// Dropping this field stops the cpal capture callback.
    stream: cpal::Stream,
    /// Shared PCM buffer populated by the capture callback (mono i16).
    pub samples: Arc<Mutex<Vec<i16>>>,
    /// The sample format the device actually opened (for informational use).
    pub sample_format: cpal::SampleFormat,
    /// Actual sample rate negotiated with the device.
    pub actual_rate: u32,
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Convert an f32 sample in `[-1.0, 1.0]` to i16.
#[inline]
fn f32_to_i16(s: f32) -> i16 {
    (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Push a mono i16 sample into the shared buffer (best-effort; ignores poison).
#[inline]
fn push_mono(buf: &Arc<Mutex<Vec<i16>>>, mono: i16) {
    if let Ok(mut guard) = buf.lock() {
        guard.push(mono);
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Open a cpal input stream and begin recording immediately.
///
/// * `device_index` — if `Some(idx)`, selects the `idx`-th device returned by
///   `host.input_devices()`; otherwise uses the system default.
///
/// Returns an [`AudioHandle`] without blocking.  The cpal callback runs on a
/// background thread managed by cpal.
pub fn start_recording(device_index: Option<usize>) -> Result<AudioHandle> {
    let host = cpal::default_host();

    // ── 1. Select device ──────────────────────────────────────────────────────
    let device = match device_index {
        Some(idx) => host
            .input_devices()
            .context("while enumerating input devices")?
            .nth(idx)
            .with_context(|| format!("no input device at index {idx}"))?,
        None => host
            .default_input_device()
            .context("no default input device found")?,
    };

    // ── 2. Negotiate config: prefer 16 kHz mono ───────────────────────────────
    let supported_configs = device
        .supported_input_configs()
        .context("while querying supported input configs")?;

    let mut best: Option<cpal::SupportedStreamConfig> = None;
    let mut best_dist = u32::MAX;

    for range in supported_configs {
        let min = range.min_sample_rate().0;
        let max = range.max_sample_rate().0;

        let clamped = TARGET_HZ.clamp(min, max);
        let dist = clamped.abs_diff(TARGET_HZ);

        let is_mono = range.channels() == 1;
        let incumbent_is_mono = best.as_ref().map(|c| c.channels() == 1).unwrap_or(false);

        // Never downgrade from a mono incumbent to multi-channel.
        if incumbent_is_mono && !is_mono {
            continue;
        }

        if dist < best_dist || (!incumbent_is_mono && is_mono) {
            best_dist = dist;
            best = Some(range.with_sample_rate(cpal::SampleRate(clamped)));
        }
    }

    let stream_config = best.context("no supported input config found on device")?;

    let sample_format = stream_config.sample_format();
    let actual_channels = stream_config.channels() as usize;
    let actual_rate = stream_config.sample_rate().0;

    tracing::debug!(
        sample_format = ?sample_format,
        channels = actual_channels,
        sample_rate = actual_rate,
        "audio::start_recording — negotiated input config"
    );

    // ── 3. Shared PCM buffer ──────────────────────────────────────────────────
    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));

    let err_fn = |e: cpal::StreamError| {
        tracing::error!("cpal stream error: {}", e);
    };

    let config: cpal::StreamConfig = stream_config.config();

    // ── 4. Build stream — one arm per sample format ───────────────────────────
    let stream = match sample_format {
        SampleFormat::I16 => {
            let buf = Arc::clone(&samples);
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    for frame in data.chunks(actual_channels) {
                        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                        let mono = (sum / actual_channels as i32)
                            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        push_mono(&buf, mono);
                    }
                },
                err_fn,
                None,
            )
        }
        SampleFormat::F32 => {
            let buf = Arc::clone(&samples);
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
            let buf = Arc::clone(&samples);
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    for frame in data.chunks(actual_channels) {
                        let sum: i32 = frame.iter().map(|&s| s as i32 - 32_768).sum();
                        let mono = (sum / actual_channels as i32)
                            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        push_mono(&buf, mono);
                    }
                },
                err_fn,
                None,
            )
        }
        other => {
            anyhow::bail!("unsupported sample format: {:?}", other);
        }
    }
    .context("while building cpal input stream")?;

    // ── 5. Start capture (non-blocking) ──────────────────────────────────────
    stream.play().context("while starting cpal input stream")?;

    Ok(AudioHandle {
        stream,
        samples,
        sample_format,
        actual_rate,
    })
}

impl AudioHandle {
    /// Stop recording, encode the buffered PCM to a temp WAV, and return its path.
    ///
    /// Drops the [`cpal::Stream`] (ending capture), resamples to 16 kHz using
    /// nearest-neighbour if the device ran at a different rate, then writes a
    /// 16 kHz mono 16-bit PCM WAV to `$TMPDIR/yaptap_<unix_timestamp>.wav`.
    pub fn stop_and_save(self) -> Result<PathBuf> {
        // ── 1. Stop capture — drop the stream ─────────────────────────────────
        drop(self.stream);

        // ── 2. Snapshot the PCM buffer ────────────────────────────────────────
        let pcm_samples: Vec<i16> = {
            let guard = self.samples.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };

        // ── 3. Nearest-neighbour resample to TARGET_HZ if needed ──────────────
        let input_len = pcm_samples.len();
        let resampled: Vec<i16> = if self.actual_rate == TARGET_HZ || pcm_samples.is_empty() {
            pcm_samples
        } else {
            let out_len = (input_len as f64 * TARGET_HZ as f64
                / self.actual_rate as f64) as usize;
            (0..out_len)
                .map(|i| {
                    let src_idx =
                        (i as f64 * self.actual_rate as f64 / TARGET_HZ as f64) as usize;
                    pcm_samples[src_idx.min(input_len - 1)]
                })
                .collect()
        };

        tracing::debug!(
            input_samples = input_len,
            output_samples = resampled.len(),
            actual_rate = self.actual_rate,
            target_rate = TARGET_HZ,
            "audio::stop_and_save — resampled"
        );

        // ── 4. Build temp WAV path ────────────────────────────────────────────
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let wav_path: PathBuf = std::env::temp_dir().join(format!("yaptap_{timestamp}.wav"));

        // ── 5. Encode with hound ──────────────────────────────────────────────
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
                writer
                    .write_sample(s)
                    .context("while writing WAV sample")?;
            }
            writer.finalize().context("while finalizing WAV file")?;
        }

        tracing::debug!(path = ?wav_path, samples = resampled.len(), "WAV written");

        Ok(wav_path)
    }
}
