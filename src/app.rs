#![allow(unexpected_cfgs)]
// YapTap — Phase 3 menu-bar application
///
/// Owns the NSApplication lifecycle, tray icon, menu, global hotkey listener,
/// and the record → transcribe → LLM → clipboard pipeline.
///
/// NOTE: `#[macro_use] extern crate objc` must be declared in `main.rs` (the
/// crate root) so that `msg_send!` / `sel!` are available here.
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::CString;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, RwLock,
};
use std::thread;

use cocoa::base::id;

use crate::audio::AudioHandle;
use crate::config::AppConfig;

// ── Icon bytes (embedded at compile time) ─────────────────────────────────────

const ICON_IDLE_1X: &[u8] = include_bytes!("../assets/icons/yaptap-idle.png");
#[allow(dead_code)]
const ICON_IDLE_2X: &[u8] = include_bytes!("../assets/icons/yaptap-idle@2x.png");
const ICON_ACTIVE_1X: &[u8] = include_bytes!("../assets/icons/yaptap-active.png");
#[allow(dead_code)]
const ICON_ACTIVE_2X: &[u8] = include_bytes!("../assets/icons/yaptap-active@2x.png");

// ── Public event enum ─────────────────────────────────────────────────────────

pub enum AppEvent {
    HotkeyPressed,
    PipelineDone(Result<String, String>),
    HotkeyError(String),
}

// ── Internal state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    Idle,
    Recording,
    Processing,
}

#[allow(clippy::arc_with_non_send_sync)]
struct SharedState {
    state: Arc<Mutex<AppState>>,
    audio: Arc<Mutex<Option<AudioHandle>>>,
    child: Arc<Mutex<Option<Child>>>,
    /// Set to true when recording starts, cleared when the pipeline takes the
    /// AudioHandle.  Guards the on_error closure so spurious post-stop errors
    /// from a racing cpal callback are silently dropped (P6-I006).
    recording_active: Arc<AtomicBool>,
    /// The currently active parsed hotkey, shared with the rdev callback via
    /// Arc<RwLock> so in-app hotkey changes take effect immediately without
    /// restarting the rdev thread (P7-I012).
    parsed_hotkey: Arc<RwLock<crate::hotkey::ParsedHotkey>>,
}

impl SharedState {
    #[allow(clippy::arc_with_non_send_sync)]
    fn new() -> Self {
        let default_parsed = crate::hotkey::parse_hotkey("option+space")
            .expect("default hotkey is always valid");
        Self {
            state: Arc::new(Mutex::new(AppState::Idle)),
            audio: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            recording_active: Arc::new(AtomicBool::new(false)),
            parsed_hotkey: Arc::new(RwLock::new(default_parsed)),
        }
    }
}

// cpal::Stream on macOS/CoreAudio is !Send due to a conservative blanket
// restriction, but the underlying CoreAudio objects are thread-safe.  We only
// ever access the AudioHandle through a Mutex, so it is safe to send the guard
// across threads.
//
// SAFETY: AudioHandle is always accessed exclusively through a Mutex, so
// concurrent access is prevented.  Sending the Arc<Mutex<Option<AudioHandle>>>
// to the pipeline thread (which then takes the handle out before stopping it)
// is sound.
unsafe impl Send for SharedState {}
unsafe impl Sync for SharedState {}

// ── Menu types ────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum MenuAction {
    ToggleRecording,
    SelectPrompt(String),
    NoPrompt,
    ChangeHotkey,
    OpenConfig,
    Quit,
}

struct PromptEntry {
    stem: String,
    name: String,
}

// ── WAV cleanup guard ─────────────────────────────────────────────────────────

struct WavCleanup(PathBuf);

impl Drop for WavCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ── Ollama availability probe ─────────────────────────────────────────────────

fn ollama_available() -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().unwrap(),
        Duration::from_secs(1),
    )
    .is_ok()
}

// ── NSAlert helper ────────────────────────────────────────────────────────────

/// Show a modal NSAlert dialog.  Returns the zero-based index of the button
/// the user clicked (0 = first/default button).
pub fn show_alert(_title: &str, message: &str, buttons: &[&str]) -> usize {
    // SAFETY: All Cocoa object pointers are obtained from documented AppKit
    // factory methods (NSAlert +new, NSString +alloc / -initWithUTF8String:)
    // that are guaranteed non-null for these calls.  This function is only
    // ever called from the main thread, satisfying AppKit's threading
    // requirement for UI operations.
    unsafe {
        let alert: id = msg_send![objc::class!(NSAlert), new];

        let msg_cstr = CString::new(message).unwrap_or_default();
        let msg_ns: id = msg_send![objc::class!(NSString), alloc];
        let msg_ns: id = msg_send![msg_ns, initWithUTF8String: msg_cstr.as_ptr()];
        let () = msg_send![alert, setMessageText: msg_ns];

        for btn in buttons {
            let btn_cstr = CString::new(*btn).unwrap_or_default();
            let btn_ns: id = msg_send![objc::class!(NSString), alloc];
            let btn_ns: id = msg_send![btn_ns, initWithUTF8String: btn_cstr.as_ptr()];
            let () = msg_send![alert, addButtonWithTitle: btn_ns];
        }

        let response: isize = msg_send![alert, runModal];
        ((response - 1000).max(0)) as usize
    }
}

// ── Single-instance guard ─────────────────────────────────────────────────────

fn lock_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/yaptap/yaptap.lock")
}

fn remove_lock_file() {
    let _ = std::fs::remove_file(lock_path());
}

fn ensure_single_instance() {
    let lock_path = lock_path();
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if lock_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&lock_path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                let alive = std::process::Command::new("kill")
                    .arg("-0")
                    .arg(pid.to_string())
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if alive {
                    std::process::exit(0);
                }
            }
        }
    }
    let our_pid = std::process::id().to_string();
    let _ = std::fs::write(&lock_path, our_pid.as_bytes());
}

// ── First-launch Python setup ─────────────────────────────────────────────────

fn run_setup_commands() -> Result<(), String> {
    let venv_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/yaptap/.venv");

    let status = std::process::Command::new("python3")
        .args(["-m", "venv"])
        .arg(&venv_dir)
        .env("PATH", crate::config::brew_augmented_path())
        .status()
        .map_err(|e| format!("failed to spawn python3: {e}"))?;
    if !status.success() {
        return Err("python3 -m venv exited non-zero".to_string());
    }

    let pip = venv_dir.join("bin/pip");
    let status = std::process::Command::new(pip)
        .args(["install", "--quiet", "openai-whisper", "ollama"])
        .status()
        .map_err(|e| format!("failed to spawn pip: {e}"))?;
    if !status.success() {
        return Err("pip install exited non-zero".to_string());
    }

    Ok(())
}

fn run_first_launch_setup() {
    // SAFETY: This function is only ever called from the main thread.
    // NSAlert and NSWindow manipulations require the main thread.
    let setup_window: id = unsafe {
        let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
        // LSUIElement = true suppresses automatic front-of-stack promotion.
        let _: () = msg_send![app, activateIgnoringOtherApps: 1u8];

        let alert: id = msg_send![objc::class!(NSAlert), new];
        let msg_cstr = CString::new(
            "Setting up YapTap\u{2026}\n\nInstalling Python dependencies. This takes about 30 seconds.",
        )
        .unwrap_or_default();
        let msg_ns: id = msg_send![objc::class!(NSString), alloc];
        let msg_ns: id = msg_send![msg_ns, initWithUTF8String: msg_cstr.as_ptr()];
        let (): () = msg_send![alert, setMessageText: msg_ns];

        // Show the alert window non-modally (no buttons — auto-dismissed when setup completes).
        let window: id = msg_send![alert, window];
        let (): () = msg_send![window, orderFront: cocoa::base::nil];
        window
    };

    // Run venv creation and pip install in a background thread.
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let _ = tx.send(run_setup_commands());
    });

    // Pump the NSApp event loop until setup completes.
    let setup_result: Result<(), String> = unsafe {
        let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
        loop {
            let date: id = msg_send![
                objc::class!(NSDate),
                dateWithTimeIntervalSinceNow: 0.016_f64
            ];
            let mode: id = msg_send![
                objc::class!(NSString),
                stringWithUTF8String: c"kCFRunLoopDefaultMode".as_ptr()
            ];
            let event: id = msg_send![
                app,
                nextEventMatchingMask: u64::MAX
                untilDate: date
                inMode: mode
                dequeue: 1u8
            ];
            if event != cocoa::base::nil {
                let _: () = msg_send![app, sendEvent: event];
            }
            if let Ok(result) = rx.try_recv() {
                break result;
            }
        }
    };

    // Dismiss the setup alert.
    unsafe {
        let _: () = msg_send![setup_window, orderOut: cocoa::base::nil];
    }

    // Handle setup outcome.
    match setup_result {
        Ok(()) => {
            // Check for ffmpeg after setup succeeds.
            let ffmpeg_ok = std::process::Command::new("ffmpeg")
                .arg("-version")
                .env("PATH", crate::config::brew_augmented_path())
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ffmpeg_ok {
                show_alert(
                    "ffmpeg not found",
                    "YapTap requires ffmpeg for audio processing.\nInstall it with: brew install ffmpeg",
                    &["OK"],
                );
            }
        }
        Err(msg) => {
            tracing::warn!(error = %msg, "first-launch setup failed");
            show_alert(
                "Setup failed",
                "YapTap could not install Python dependencies. Ensure python3 is installed and try launching again.",
                &["OK"],
            );
        }
    }
}

// ── Graceful exit (P3-T067) ───────────────────────────────────────────────────

fn cleanup_and_exit(shared: &SharedState) -> ! {
    // Kill any active subprocess.
    if let Ok(mut guard) = shared.child.lock() {
        if let Some(ref mut child) = *guard {
            let _ = child.kill();
        }
    }
    // Stop any in-progress audio capture.
    if let Ok(mut guard) = shared.audio.lock() {
        if let Some(handle) = guard.take() {
            let _ = handle.stop_and_save();
        }
    }
    remove_lock_file();
    std::process::exit(0);
}

// ── Icon loading ──────────────────────────────────────────────────────────────

fn load_icon(bytes: &[u8]) -> tray_icon::Icon {
    let img = image::load_from_memory(bytes).expect("failed to decode icon PNG");
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), w, h).expect("failed to create tray icon")
}

fn switch_icon(tray: &tray_icon::TrayIcon, state: &AppState) {
    let bytes = match state {
        AppState::Idle => ICON_IDLE_1X,
        AppState::Recording | AppState::Processing => ICON_ACTIVE_1X,
    };
    if let Ok(img) = image::load_from_memory(bytes) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        if let Ok(icon) = tray_icon::Icon::from_rgba(rgba.into_raw(), w, h) {
            let _ = tray.set_icon(Some(icon));
        }
    }
}

// ── Menu building ─────────────────────────────────────────────────────────────

fn hotkey_display(hotkey: &str) -> String {
    let s = hotkey
        .replace("option", "\u{2325}") // ⌥
        .replace("cmd", "\u{2318}")    // ⌘
        .replace("ctrl", "\u{2303}")   // ⌃
        .replace("shift", "\u{21E7}")  // ⇧
        .replace("space", "Space")
        .replace('+', "");
    // Uppercase any remaining ASCII letter (the main key character).
    s.chars()
        .map(|c| if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c })
        .collect()
}

fn load_prompts() -> Vec<PromptEntry> {
    let dir = match crate::config::prompts_dir() {
        Some(d) if d.is_dir() => d,
        _ => return Vec::new(),
    };

    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            let name = if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                    val.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&stem)
                        .to_string()
                } else {
                    stem.clone()
                }
            } else {
                stem.clone()
            };
            entries.push(PromptEntry { stem, name });
        }
    }

    entries.sort_by(|a, b| a.stem.cmp(&b.stem));
    entries
}

fn build_menu(
    prompts: &[PromptEntry],
    config: &AppConfig,
    state: &AppState,
) -> (
    tray_icon::menu::Menu,
    HashMap<tray_icon::menu::MenuId, MenuAction>,
) {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    let mut actions: HashMap<tray_icon::menu::MenuId, MenuAction> = HashMap::new();

    // ── Recording toggle (P3-T076) ────────────────────────────────────────────
    let (record_label, record_enabled) = match state {
        AppState::Idle => ("Start Recording", true),
        AppState::Recording => ("Stop Recording", true),
        AppState::Processing => ("Processing\u{2026}", false),
    };
    let record_item = MenuItem::new(record_label, record_enabled, None);
    if record_enabled {
        actions.insert(record_item.id().clone(), MenuAction::ToggleRecording);
    }
    if let Err(e) = menu.append(&record_item) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }
    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    // ── Prompt list ───────────────────────────────────────────────────────────
    if prompts.is_empty() {
        let no_prompts = MenuItem::new("No prompts found", false, None);
        if let Err(e) = menu.append(&no_prompts) {
            tracing::warn!(error = ?e, "failed to append menu item");
        }
    } else {
        for entry in prompts {
            let checked = entry.stem == config.selected_prompt;
            let item = CheckMenuItem::new(&entry.name, true, checked, None);
            actions.insert(item.id().clone(), MenuAction::SelectPrompt(entry.stem.clone()));
            if let Err(e) = menu.append(&item) {
                tracing::warn!(error = ?e, "failed to append menu item");
            }
        }
    }

    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    // ── "No Prompt" option ────────────────────────────────────────────────────
    let no_prompt_checked = config.selected_prompt.is_empty();
    let no_prompt = CheckMenuItem::new("No Prompt", true, no_prompt_checked, None);
    actions.insert(no_prompt.id().clone(), MenuAction::NoPrompt);
    if let Err(e) = menu.append(&no_prompt) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    // ── Hotkey display / change (P7-I014) ────────────────────────────────────
    let hotkey_text = format!("Hotkey: {}", hotkey_display(&config.hotkey));
    let hotkey_item = MenuItem::new(&hotkey_text, true, None);
    actions.insert(hotkey_item.id().clone(), MenuAction::ChangeHotkey);
    if let Err(e) = menu.append(&hotkey_item) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    // ── Open Config… ──────────────────────────────────────────────────────────
    let open_config = MenuItem::new("Open Config\u{2026}", true, None);
    actions.insert(open_config.id().clone(), MenuAction::OpenConfig);
    if let Err(e) = menu.append(&open_config) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    if let Err(e) = menu.append(&PredefinedMenuItem::separator()) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    // ── Quit ──────────────────────────────────────────────────────────────────
    let quit = MenuItem::new("Quit YapTap", true, None);
    actions.insert(quit.id().clone(), MenuAction::Quit);
    if let Err(e) = menu.append(&quit) {
        tracing::warn!(error = ?e, "failed to append menu item");
    }

    (menu, actions)
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

fn run_pipeline_thread(
    shared: Arc<SharedState>,
    config: AppConfig,
    sender: Arc<mpsc::Sender<AppEvent>>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_pipeline_inner(&shared, &config)
    }));

    let event = match result {
        Ok(Ok(output)) => AppEvent::PipelineDone(Ok(output)),
        Ok(Err(e)) => AppEvent::PipelineDone(Err(e.to_string())),
        Err(_) => AppEvent::PipelineDone(Err("internal error: pipeline panicked".to_string())),
    };
    let _ = sender.send(event);
}

fn run_pipeline_inner(shared: &SharedState, config: &AppConfig) -> anyhow::Result<String> {
    // ── Pre-flight checks ─────────────────────────────────────────────────────
    let py_ok = std::process::Command::new("python3")
        .arg("--version")
        .env("PATH", crate::config::brew_augmented_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !py_ok {
        anyhow::bail!("python3 not found on PATH");
    }

    let ff_ok = std::process::Command::new("ffmpeg")
        .arg("-version")
        .env("PATH", crate::config::brew_augmented_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ff_ok {
        anyhow::bail!("ffmpeg not found on PATH");
    }

    // ── Take AudioHandle from shared state ────────────────────────────────────
    let handle = {
        let mut guard = shared
            .audio
            .lock()
            .map_err(|_| anyhow::anyhow!("audio mutex poisoned"))?;
        guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("no audio handle found"))?
    };

    let wav_path = handle.stop_and_save()?;

    // Cleanup guard — deletes the WAV when this scope exits.
    let _wav_guard = WavCleanup(wav_path.clone());

    // ── Transcribe ────────────────────────────────────────────────────────────
    let transcript = crate::transcription::run_transcription(
        &wav_path,
        &config.whisper_model,
        &shared.child,
    )?;

    // ── LLM (if a prompt is selected) ────────────────────────────────────────
    let output = if !config.selected_prompt.is_empty() {
        // Probe Ollama availability before attempting the LLM step.
        if !ollama_available() {
            anyhow::bail!(
                "Ollama not running. Start Ollama and try again.\n\
                 Run `ollama serve` in a terminal, or open the Ollama app."
            );
        }
        if let Some(prompts_dir) = crate::config::prompts_dir() {
            let prompt_path = prompts_dir.join(format!("{}.toml", config.selected_prompt));
            crate::llm::run_llm_collect(&transcript, &prompt_path, &config.llm_model, &shared.child)?
        } else {
            transcript
        }
    } else {
        transcript
    };

    Ok(output)
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_app() {
    // ── Configure NSApplication as a menu-bar-only (accessory) process ──────────
    // NSApplicationActivationPolicyAccessory (1): no Dock icon, no app menu bar,
    // but the app CAN receive menu-bar clicks and other UI events.
    // NSApplicationActivationPolicyProhibited (2) is wrong here — it prevents
    // any user-interface event processing (menus never open).
    //
    // SAFETY: sharedApplication and setActivationPolicy: are documented
    // NSApplication methods safe to call on the main thread before any windows
    // exist.  finishLaunching completes Cocoa's internal setup and is required
    // before any UI (including NSAlert) can be shown.
    unsafe {
        let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: 1i64];
        // finishLaunching completes Cocoa's setup; without it menu delegates
        // are never installed and click events are silently dropped.
        let _: () = msg_send![app, finishLaunching];
    }

    // ── Load config ───────────────────────────────────────────────────────────
    let (mut config, config_warnings) = AppConfig::load();

    // Show any deferred config warnings (TOML parse errors, invalid hotkey)
    // as UI alerts now that finishLaunching has been called.
    for warning in config_warnings {
        show_alert("Configuration Warning", &warning, &["OK"]);
    }

    // ── Single-instance guard (may exit 0 if another copy is running) ─────────
    ensure_single_instance();

    // ── First-launch Python venv setup ────────────────────────────────────────
    // Must be after ensure_single_instance() so the alert is never shown to a
    // duplicate instance that is about to exit 0.
    {
        let venv_python = dirs::home_dir()
            .unwrap_or_default()
            .join(".config/yaptap/.venv/bin/python");
        if !venv_python.is_file() {
            run_first_launch_setup();
        }
    }

    // ── Shared state ──────────────────────────────────────────────────────────
    let shared = Arc::new(SharedState::new());

    // ── SIGTERM / SIGINT handler ──────────────────────────────────────────────
    // cpal::Stream is !Send so SharedState cannot be captured by the signal
    // handler thread.  Instead we set an AtomicBool; the main event loop checks
    // it each iteration and calls cleanup_and_exit() (which kills the child
    // process, stops audio, removes the lock file, and exits).
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&shutdown_requested);
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    // ── App-event channel ─────────────────────────────────────────────────────
    let (app_event_tx, app_event_rx) = mpsc::channel::<AppEvent>();
    let app_event_tx = Arc::new(app_event_tx);

    // ── Accessibility check + global hotkey thread ────────────────────────────
    if !crate::hotkey::ax_is_process_trusted() {
        let btn = show_alert(
            "Accessibility Required",
            "YapTap needs Accessibility access to capture the global hotkey.\n\nOpen System Settings \u{2192} Privacy & Security \u{2192} Accessibility?\n\nAfter granting permission, quit and relaunch YapTap to activate the hotkey.",
            &["Open Settings", "Later"],
        );
        if btn == 0 {
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .spawn();
        }
    } else {
        match crate::hotkey::parse_hotkey(&config.hotkey) {
            Ok(parsed) => {
                // Store the actual parsed hotkey in shared state so the main
                // thread can update it live for in-app hotkey changes (P7-I012).
                *shared.parsed_hotkey.write().unwrap() = parsed;
                let hotkey_arc = Arc::clone(&shared.parsed_hotkey);
                let tx = Arc::clone(&app_event_tx);
                let hotkey_str = config.hotkey.clone();
                thread::spawn(move || {
                    tracing::debug!("rdev thread spawned");
                    // tx_inner is moved into the rdev callback; tx is kept for
                    // the error path after rdev::listen() returns.
                    let tx_inner = Arc::clone(&tx);
                    let mut pressed: HashSet<rdev::Key> = HashSet::new();
                    // P5-I008: tracks whether the main key is currently held,
                    // suppressing key-repeat events from firing multiple hotkeys.
                    let mut main_key_down = false;
                    // P7-I009: one-shot log confirming the CGEventTap is live.
                    // Declared here (thread::spawn closure scope) so it can be
                    // referenced inside the nested rdev callback closure.
                    static LISTEN_STARTED: std::sync::Once = std::sync::Once::new();
                    let result = rdev::listen(move |event| {
                        // P7-I009: fires on the very first event of any type,
                        // confirming the tap has started receiving events.
                        LISTEN_STARTED.call_once(|| tracing::info!("rdev event tap active"));
                        match event.event_type {
                            rdev::EventType::KeyPress(key) => {
                                pressed.insert(key);
                                // P5-I007: treat AltGr (right ⌥Option, hardware
                                // keycode 61) as equivalent to Alt (left ⌥Option,
                                // keycode 58) when testing modifier satisfaction.
                                let mut effective_pressed = pressed.clone();
                                if effective_pressed.contains(&rdev::Key::AltGr) {
                                    effective_pressed.insert(rdev::Key::Alt);
                                }
                                // P7-I012: read parsed hotkey through RwLock so
                                // in-app changes take effect immediately.
                                let parsed = hotkey_arc.read().unwrap();
                                let modifiers_held = parsed
                                    .modifiers
                                    .iter()
                                    .all(|m| effective_pressed.contains(m));
                                // P5-I009: fire only when the main key is the
                                // key just pressed (not when a modifier is pressed
                                // onto an already-held main key).
                                // P5-I008: suppress auto-repeat by gating on
                                // !main_key_down.
                                if modifiers_held && key == parsed.key && !main_key_down {
                                    main_key_down = true;
                                    let _ = tx_inner.send(AppEvent::HotkeyPressed);
                                }
                            }
                            rdev::EventType::KeyRelease(key) => {
                                pressed.remove(&key);
                                // P7-I007: reset main_key_down when the pressed
                                // set becomes empty (all keys released).  This
                                // recovers from a missed KeyRelease event that
                                // would otherwise leave main_key_down stuck true.
                                if pressed.is_empty() {
                                    main_key_down = false;
                                }
                            }
                            _ => {}
                        }
                    });
                    if let Err(e) = result {
                        tracing::error!("rdev listen error: {e:?}");
                        let _ = tx.send(AppEvent::HotkeyError(format!(
                            "The hotkey '{hotkey_str}' could not be registered. \
                             Edit ~/.config/yaptap/config.toml to choose a different \
                             hotkey, then restart YapTap."
                        )));
                    }
                });
            }
            Err(e) => {
                tracing::error!("could not parse hotkey {:?}: {}", config.hotkey, e);
            }
        }
    }

    // ── Build tray icon ───────────────────────────────────────────────────────
    let idle_icon = load_icon(ICON_IDLE_1X);
    let prompts = load_prompts();
    let (menu, mut menu_actions) = build_menu(&prompts, &config, &AppState::Idle);

    let tray = tray_icon::TrayIconBuilder::new()
        .with_icon(idle_icon)
        .with_menu(Box::new(menu))
        .with_tooltip("YapTap")
        .build()
        .expect("failed to create tray icon");

    // FIXME(P5-I002): template image flag not set — tray-icon 0.14 exposes no
    // public API for marking the NSStatusItem's NSImage as a template image.
    // Icons are black-on-transparent and will be invisible on a dark menu bar
    // until tray-icon adds template image support or a safe alternative is found.
    // The previous implementation (P3-T034) used the private NSSystemStatusBar
    // -statusItems selector which caused NSInvalidArgumentException on launch
    // and has been removed (P5-I001).

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        // Check if a signal handler has requested shutdown.  cleanup_and_exit()
        // kills any active subprocess, stops audio capture, removes the lock
        // file, and calls process::exit(0).
        if shutdown_requested.load(Ordering::SeqCst) {
            cleanup_and_exit(&shared);
        }

        // Each iteration gets its own autorelease pool — Cocoa allocates many
        // temporary objects during event processing that must be drained promptly.
        //
        // SAFETY: NSAutoreleasePool +new is a documented factory method that
        // returns a valid pool for the current thread.  The pool is drained at
        // the bottom of this loop iteration on the same thread it was created on,
        // which is required by Cocoa's autorelease pool semantics.
        let pool: id = unsafe { msg_send![objc::class!(NSAutoreleasePool), new] };

        // Drain the NSApplication NSEvent queue.
        //
        // NSRunLoop::runUntilDate does NOT pump the NSApplication event queue —
        // NSStatusBar click events are NSEvents delivered to the app, not raw
        // CFRunLoop sources.  We must call nextEventMatchingMask:untilDate:
        // inMode:dequeue: + sendEvent: to dispatch them to tray-icon's NSView
        // (TaoTrayTarget), which is what fires the menu and TrayIconEvent channel.
        //
        // SAFETY: sharedApplication returns the singleton NSApplication created
        // above; it is never null after +sharedApplication.  All msg_send! calls
        // here execute on the main thread as required by AppKit.  The event
        // pointer returned by nextEventMatchingMask:... is either nil (timeout)
        // or a valid autoreleased NSEvent owned by the run loop; we test for nil
        // before calling sendEvent:.
        unsafe {
            let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
            // Wait up to 16 ms for the next event; returns nil on timeout.
            let date: id =
                msg_send![objc::class!(NSDate), dateWithTimeIntervalSinceNow: 0.016_f64];
            let mode: id = msg_send![
                objc::class!(NSString),
                stringWithUTF8String: c"kCFRunLoopDefaultMode".as_ptr()
            ];
            let event: id = msg_send![
                app,
                nextEventMatchingMask: u64::MAX
                untilDate: date
                inMode: mode
                dequeue: 1u8  // YES
            ];
            if event != cocoa::base::nil {
                let _: () = msg_send![app, sendEvent: event];
                let _: () = msg_send![app, updateWindows];
            }
        }

        // ── MenuEvent (user chose a menu item) ───────────────────────────────
        // Drained BEFORE TrayIconEvent so that a menu-item click is dispatched
        // with the IDs that were live when the menu opened.  TrayIconEvent
        // rebuilds the menu (replacing all MenuIds); if it ran first, any
        // pending MenuEvent would look up a stale ID and be silently dropped
        // (P6-I001 root cause).
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            tracing::debug!(menu_id = ?event.id, "MenuEvent received");
            tracing::debug!(menu_id = ?event.id, found = menu_actions.contains_key(&event.id), "MenuEvent dispatch");
            if let Some(action) = menu_actions.get(&event.id).cloned() {
                match action {
                    MenuAction::ToggleRecording => {
                        // Delegate to the existing state machine — identical to hotkey.
                        let _ = app_event_tx.send(AppEvent::HotkeyPressed);
                    }
                    MenuAction::SelectPrompt(stem) => {
                        config.selected_prompt = stem.clone();
                        let _ = config.save_prompt(&stem);
                        let cur_state = shared.state.lock().unwrap().clone();
                        rebuild_menu(&tray, &mut menu_actions, &config, &cur_state);
                    }
                    MenuAction::NoPrompt => {
                        config.selected_prompt = String::new();
                        let _ = config.save_prompt("");
                        let cur_state = shared.state.lock().unwrap().clone();
                        rebuild_menu(&tray, &mut menu_actions, &config, &cur_state);
                    }
                    MenuAction::ChangeHotkey => {
                        // P7-I014: show input dialog, validate, apply live, persist.
                        if let Some(new_hotkey) = prompt_hotkey_change(&config.hotkey) {
                            match crate::hotkey::parse_hotkey(&new_hotkey) {
                                Ok(new_parsed) => {
                                    *shared.parsed_hotkey.write().unwrap() = new_parsed;
                                    let _ = config.save_hotkey(&new_hotkey);
                                    config.hotkey = new_hotkey;
                                    let cur_state = shared.state.lock().unwrap().clone();
                                    rebuild_menu(&tray, &mut menu_actions, &config, &cur_state);
                                }
                                Err(_) => {
                                    show_alert(
                                        "Invalid Hotkey",
                                        &format!(
                                            "Invalid hotkey: {new_hotkey}. Use format option+space or cmd+shift+y."
                                        ),
                                        &["OK"],
                                    );
                                }
                            }
                        }
                    }
                    MenuAction::OpenConfig => {
                        let _ = std::process::Command::new("open")
                            .arg(crate::config::config_path())
                            .spawn();
                    }
                    MenuAction::Quit => {
                        cleanup_and_exit(&shared);
                    }
                }
            }
        }

        // ── TrayIconEvent (icon clicked) ──────────────────────────────────────
        // Drained AFTER MenuEvent so that menu ID lookups above use the IDs
        // from the currently displayed menu.  Rebuilds the menu to refresh
        // prompt check marks and the recording label (P6-I001).
        while let Ok(_event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            tracing::debug!("TrayIconEvent received");
            // Rebuild the menu so check marks and recording label reflect current state.
            let prompts = load_prompts();
            let cur_state = shared.state.lock().unwrap().clone();
            let (new_menu, new_actions) = build_menu(&prompts, &config, &cur_state);
            menu_actions = new_actions;
            tray.set_menu(Some(Box::new(new_menu)));
        }

        // ── AppEvent (hotkey / pipeline result) ───────────────────────────────
        while let Ok(event) = app_event_rx.try_recv() {
            match event {
                AppEvent::HotkeyPressed => {
                    let state = shared.state.lock().unwrap().clone();
                    tracing::info!(state = ?state, "HotkeyPressed");
                    match state {
                        AppState::Idle => {
                            // Start recording.  Pass an error callback so that
                            // cpal stream errors (e.g. device disconnect) are
                            // forwarded to the main event loop as pipeline errors
                            // (P3-T069).  Guard the closure with recording_active
                            // so a racing callback after stop does not fire a
                            // spurious error alert (P6-I006).
                            let err_tx = Arc::clone(&app_event_tx);
                            let recording_active = Arc::clone(&shared.recording_active);
                            match crate::audio::start_recording(None, move |e| {
                                if recording_active.load(Ordering::SeqCst) {
                                    let _ = err_tx.send(AppEvent::PipelineDone(Err(
                                        format!("audio device error: {e}"),
                                    )));
                                }
                            }) {
                                Ok(handle) => {
                                    shared.recording_active.store(true, Ordering::SeqCst);
                                    *shared.audio.lock().unwrap() = Some(handle);
                                    *shared.state.lock().unwrap() = AppState::Recording;
                                    switch_icon(&tray, &AppState::Recording);
                                    // Sync menu label to "Stop Recording" immediately
                                    // so it is correct if opened via hotkey (P6-I002).
                                    rebuild_menu(&tray, &mut menu_actions, &config, &AppState::Recording);
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "start_recording failed");
                                    // Provide actionable guidance when the error
                                    // looks like a TCC microphone denial (P6-I004).
                                    let err_str = e.to_string();
                                    let msg = if err_str.contains("561015905")
                                        || err_str.contains("not permitted")
                                        || err_str.contains("kAudioHardwareNotRunningError")
                                        || err_str.contains("-536870174")
                                    {
                                        "Microphone access is required. Grant it in \
                                         System Settings \u{2192} Privacy & Security \
                                         \u{2192} Microphone, then restart YapTap."
                                            .to_string()
                                    } else {
                                        format!("Failed to start recording: {e}")
                                    };
                                    show_alert("Recording Error", &msg, &["OK"]);
                                }
                            }
                        }
                        AppState::Recording => {
                            // Clear the guard before the pipeline thread takes
                            // the AudioHandle so the on_error closure stops
                            // firing if the device disconnects during teardown
                            // (P6-I006).
                            shared.recording_active.store(false, Ordering::SeqCst);
                            // Stop recording and kick off the pipeline.
                            *shared.state.lock().unwrap() = AppState::Processing;
                            // Sync menu label to "Processing…" immediately (P6-I002).
                            rebuild_menu(&tray, &mut menu_actions, &config, &AppState::Processing);
                            // Icon stays active (orange) during processing.
                            let shared_clone = Arc::clone(&shared);
                            let config_clone = config.clone();
                            let tx_clone = Arc::clone(&app_event_tx);
                            thread::spawn(move || {
                                run_pipeline_thread(shared_clone, config_clone, tx_clone);
                            });
                        }
                        AppState::Processing => {
                            // Hotkey pressed while pipeline is running — ignore.
                        }
                    }
                }

                AppEvent::PipelineDone(result) => {
                    *shared.state.lock().unwrap() = AppState::Idle;
                    // Drop any lingering AudioHandle.  Under normal operation the
                    // pipeline thread already took it; under a stream error it may
                    // still be in shared.audio and should be released now.
                    if let Ok(mut guard) = shared.audio.lock() {
                        drop(guard.take());
                    }
                    switch_icon(&tray, &AppState::Idle);
                    // Sync menu label back to "Start Recording" (P6-I002).
                    rebuild_menu(&tray, &mut menu_actions, &config, &AppState::Idle);
                    match result {
                        Ok(output) => {
                            match arboard::Clipboard::new()
                                .and_then(|mut cb| cb.set_text(output))
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    show_alert(
                                        "Clipboard Error",
                                        &format!("Failed to copy to clipboard: {e}"),
                                        &["OK"],
                                    );
                                }
                            }
                        }
                        Err(msg) => {
                            show_alert("Pipeline Error", &msg, &["OK"]);
                        }
                    }
                }

                AppEvent::HotkeyError(msg) => {
                    show_alert("Hotkey Error", &msg, &["OK"]);
                }
            }
        }

        // Drain the per-iteration autorelease pool.
        //
        // SAFETY: pool was created by NSAutoreleasePool +new at the top of this
        // loop iteration on the same (main) thread.  Draining it here is sound
        // because all autoreleased objects from this iteration are in scope and
        // no references to them are held past this point.
        unsafe { let _: () = msg_send![pool, drain]; }
    }
}

// ── Hotkey input dialog (P7-I014) ─────────────────────────────────────────────

/// Show an NSAlert with an NSTextField pre-filled with `current`.
///
/// Returns `Some(new_value)` if the user clicked OK and entered a non-empty
/// string, or `None` if they cancelled or left the field empty.
fn prompt_hotkey_change(current: &str) -> Option<String> {
    // Local C-compatible structs for NSRect / NSPoint / NSSize.  These match
    // the AppKit layout exactly (two f64 fields each).
    #[repr(C)]
    struct NSPoint { x: f64, y: f64 }
    #[repr(C)]
    struct NSSize { width: f64, height: f64 }
    #[repr(C)]
    struct NSRect { origin: NSPoint, size: NSSize }

    // SAFETY: All Cocoa objects are obtained from documented AppKit factory
    // methods.  This function is only called from the main thread, satisfying
    // AppKit's threading requirement.
    unsafe {
        let alert: id = msg_send![objc::class!(NSAlert), new];

        let msg_cstr = CString::new(
            "Enter new hotkey (e.g. option+space, cmd+shift+y):"
        ).unwrap_or_default();
        let msg_ns: id = msg_send![objc::class!(NSString), alloc];
        let msg_ns: id = msg_send![msg_ns, initWithUTF8String: msg_cstr.as_ptr()];
        let () = msg_send![alert, setMessageText: msg_ns];

        // Create an NSTextField pre-filled with the current hotkey string.
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize { width: 220.0, height: 24.0 },
        };
        let text_field: id = msg_send![objc::class!(NSTextField), alloc];
        let text_field: id = msg_send![text_field, initWithFrame: frame];

        let current_cstr = CString::new(current).unwrap_or_default();
        let current_ns: id = msg_send![objc::class!(NSString), alloc];
        let current_ns: id = msg_send![current_ns, initWithUTF8String: current_cstr.as_ptr()];
        let () = msg_send![text_field, setStringValue: current_ns];

        let () = msg_send![alert, setAccessoryView: text_field];

        // OK button first (NSAlertFirstButtonReturn = 1000).
        let ok_cstr = CString::new("OK").unwrap_or_default();
        let ok_ns: id = msg_send![objc::class!(NSString), alloc];
        let ok_ns: id = msg_send![ok_ns, initWithUTF8String: ok_cstr.as_ptr()];
        let () = msg_send![alert, addButtonWithTitle: ok_ns];

        // Cancel button second (NSAlertSecondButtonReturn = 1001).
        let cancel_cstr = CString::new("Cancel").unwrap_or_default();
        let cancel_ns: id = msg_send![objc::class!(NSString), alloc];
        let cancel_ns: id = msg_send![cancel_ns, initWithUTF8String: cancel_cstr.as_ptr()];
        let () = msg_send![alert, addButtonWithTitle: cancel_ns];

        // Run modal and check result.
        let response: isize = msg_send![alert, runModal];
        if response != 1000 {
            return None; // Cancelled or second button
        }

        // Read text from the field.
        let value_ns: id = msg_send![text_field, stringValue];
        let value_ptr: *const std::os::raw::c_char = msg_send![value_ns, UTF8String];
        if value_ptr.is_null() {
            return None;
        }
        let value = std::ffi::CStr::from_ptr(value_ptr)
            .to_string_lossy()
            .into_owned();

        if value.is_empty() { None } else { Some(value) }
    }
}

// ── Private helper: rebuild menu and swap it into the tray ───────────────────

fn rebuild_menu(
    tray: &tray_icon::TrayIcon,
    menu_actions: &mut HashMap<tray_icon::menu::MenuId, MenuAction>,
    config: &AppConfig,
    state: &AppState,
) {
    let prompts = load_prompts();
    let (new_menu, new_actions) = build_menu(&prompts, config, state);
    *menu_actions = new_actions;
    tray.set_menu(Some(Box::new(new_menu)));
}
