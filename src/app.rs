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
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use cocoa::base::id;

use crate::audio::AudioHandle;
use crate::config::AppConfig;

// ── Icon bytes (embedded at compile time) ─────────────────────────────────────

const ICON_IDLE_1X: &[u8] = include_bytes!("../assets/icons/yaptap-idle.png");
const ICON_ACTIVE_1X: &[u8] = include_bytes!("../assets/icons/yaptap-active.png");

// ── Public event enum ─────────────────────────────────────────────────────────

pub enum AppEvent {
    HotkeyPressed,
    PipelineDone(Result<String, String>),
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
}

impl SharedState {
    #[allow(clippy::arc_with_non_send_sync)]
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::Idle)),
            audio: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
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
    SelectPrompt(String),
    NoPrompt,
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

// ── NSAlert helper ────────────────────────────────────────────────────────────

/// Show a modal NSAlert dialog.  Returns the zero-based index of the button
/// the user clicked (0 = first/default button).
pub fn show_alert(_title: &str, message: &str, buttons: &[&str]) -> usize {
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
    hotkey
        .replace("option", "\u{2325}") // ⌥
        .replace("cmd", "\u{2318}")    // ⌘
        .replace("ctrl", "\u{2303}")   // ⌃
        .replace("shift", "\u{21E7}")  // ⇧
        .replace("space", "Space")
        .replace('+', "")
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
) -> (
    tray_icon::menu::Menu,
    HashMap<tray_icon::menu::MenuId, MenuAction>,
) {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    let mut actions: HashMap<tray_icon::menu::MenuId, MenuAction> = HashMap::new();

    // ── Prompt list ───────────────────────────────────────────────────────────
    if prompts.is_empty() {
        let no_prompts = MenuItem::new("No prompts found", false, None);
        let _ = menu.append(&no_prompts);
    } else {
        for entry in prompts {
            let checked = entry.stem == config.selected_prompt;
            let item = CheckMenuItem::new(&entry.name, true, checked, None);
            actions.insert(item.id().clone(), MenuAction::SelectPrompt(entry.stem.clone()));
            let _ = menu.append(&item);
        }
    }

    let _ = menu.append(&PredefinedMenuItem::separator());

    // ── "No Prompt" option ────────────────────────────────────────────────────
    #[allow(clippy::comparison_to_empty)]
    let no_prompt_checked = config.selected_prompt == "";
    let no_prompt = CheckMenuItem::new("No Prompt", true, no_prompt_checked, None);
    actions.insert(no_prompt.id().clone(), MenuAction::NoPrompt);
    let _ = menu.append(&no_prompt);

    let _ = menu.append(&PredefinedMenuItem::separator());

    // ── Informational hotkey display (disabled) ───────────────────────────────
    let hotkey_text = format!("Hotkey: {}", hotkey_display(&config.hotkey));
    let hotkey_item = MenuItem::new(&hotkey_text, false, None);
    let _ = menu.append(&hotkey_item);

    // ── Open Config… ──────────────────────────────────────────────────────────
    let open_config = MenuItem::new("Open Config\u{2026}", true, None);
    actions.insert(open_config.id().clone(), MenuAction::OpenConfig);
    let _ = menu.append(&open_config);

    let _ = menu.append(&PredefinedMenuItem::separator());

    // ── Quit ──────────────────────────────────────────────────────────────────
    let quit = MenuItem::new("Quit YapTap", true, None);
    actions.insert(quit.id().clone(), MenuAction::Quit);
    let _ = menu.append(&quit);

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
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !py_ok {
        anyhow::bail!("python3 not found on PATH");
    }

    let ff_ok = std::process::Command::new("ffmpeg")
        .arg("-version")
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
    #[allow(clippy::comparison_to_empty)]
    let output = if config.selected_prompt != "" {
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
    unsafe {
        let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: 1i64];
        // finishLaunching completes Cocoa's setup; without it menu delegates
        // are never installed and click events are silently dropped.
        let _: () = msg_send![app, finishLaunching];
    }

    // ── Load config ───────────────────────────────────────────────────────────
    let mut config = AppConfig::load();

    // ── Single-instance guard (may exit 0 if another copy is running) ─────────
    ensure_single_instance();

    // ── Shared state ──────────────────────────────────────────────────────────
    let shared = Arc::new(SharedState::new());

    // ── SIGTERM / SIGINT handler ──────────────────────────────────────────────
    // Signal handlers must be Send; cpal::Stream is not Send, so we can't
    // capture `shared` directly.  Instead we just remove the lock file and exit.
    let _ = ctrlc::set_handler(move || {
        remove_lock_file();
        std::process::exit(0);
    });

    // ── App-event channel ─────────────────────────────────────────────────────
    let (app_event_tx, app_event_rx) = mpsc::channel::<AppEvent>();
    let app_event_tx = Arc::new(app_event_tx);

    // ── Accessibility check + global hotkey thread ────────────────────────────
    if !crate::hotkey::ax_is_process_trusted() {
        let btn = show_alert(
            "Accessibility Required",
            "YapTap needs Accessibility access to capture the global hotkey.\n\nOpen System Settings \u{2192} Privacy & Security \u{2192} Accessibility?",
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
                let tx = Arc::clone(&app_event_tx);
                thread::spawn(move || {
                    let mut pressed: HashSet<rdev::Key> = HashSet::new();
                    let result = rdev::listen(move |event| {
                        match event.event_type {
                            rdev::EventType::KeyPress(key) => {
                                pressed.insert(key);
                                let modifiers_held =
                                    parsed.modifiers.iter().all(|m| pressed.contains(m));
                                if modifiers_held && pressed.contains(&parsed.key) {
                                    let _ = tx.send(AppEvent::HotkeyPressed);
                                }
                            }
                            rdev::EventType::KeyRelease(key) => {
                                pressed.remove(&key);
                            }
                            _ => {}
                        }
                    });
                    if let Err(e) = result {
                        eprintln!("rdev listen error: {e:?}");
                    }
                });
            }
            Err(e) => {
                eprintln!("warning: could not parse hotkey {:?}: {}", config.hotkey, e);
            }
        }
    }

    // ── Build tray icon ───────────────────────────────────────────────────────
    let idle_icon = load_icon(ICON_IDLE_1X);
    let prompts = load_prompts();
    let (menu, mut menu_actions) = build_menu(&prompts, &config);

    let tray = tray_icon::TrayIconBuilder::new()
        .with_icon(idle_icon)
        .with_menu(Box::new(menu))
        .with_tooltip("YapTap")
        .build()
        .expect("failed to create tray icon");

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        // Each iteration gets its own autorelease pool — Cocoa allocates many
        // temporary objects during event processing that must be drained promptly.
        let pool: id = unsafe { msg_send![objc::class!(NSAutoreleasePool), new] };

        // Drain the NSApplication NSEvent queue.
        //
        // NSRunLoop::runUntilDate does NOT pump the NSApplication event queue —
        // NSStatusBar click events are NSEvents delivered to the app, not raw
        // CFRunLoop sources.  We must call nextEventMatchingMask:untilDate:
        // inMode:dequeue: + sendEvent: to dispatch them to tray-icon's NSView
        // (TaoTrayTarget), which is what fires the menu and TrayIconEvent channel.
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

        // ── TrayIconEvent (icon clicked) ──────────────────────────────────────
        while let Ok(_event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            // Rebuild the menu so check marks reflect the current config.
            let prompts = load_prompts();
            let (new_menu, new_actions) = build_menu(&prompts, &config);
            menu_actions = new_actions;
            tray.set_menu(Some(Box::new(new_menu)));
        }

        // ── MenuEvent (user chose a menu item) ────────────────────────────────
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if let Some(action) = menu_actions.get(&event.id).cloned() {
                match action {
                    MenuAction::SelectPrompt(stem) => {
                        config.selected_prompt = stem.clone();
                        let _ = config.save_prompt(&stem);
                        rebuild_menu(&tray, &mut menu_actions, &config);
                    }
                    MenuAction::NoPrompt => {
                        config.selected_prompt = String::new();
                        let _ = config.save_prompt("");
                        rebuild_menu(&tray, &mut menu_actions, &config);
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

        // ── AppEvent (hotkey / pipeline result) ───────────────────────────────
        while let Ok(event) = app_event_rx.try_recv() {
            match event {
                AppEvent::HotkeyPressed => {
                    let state = shared.state.lock().unwrap().clone();
                    match state {
                        AppState::Idle => {
                            // Start recording.
                            match crate::audio::start_recording(None) {
                                Ok(handle) => {
                                    *shared.audio.lock().unwrap() = Some(handle);
                                    *shared.state.lock().unwrap() = AppState::Recording;
                                    switch_icon(&tray, &AppState::Recording);
                                }
                                Err(e) => {
                                    show_alert(
                                        "Recording Error",
                                        &format!("Failed to start recording: {e}"),
                                        &["OK"],
                                    );
                                }
                            }
                        }
                        AppState::Recording => {
                            // Stop recording and kick off the pipeline.
                            *shared.state.lock().unwrap() = AppState::Processing;
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
                    switch_icon(&tray, &AppState::Idle);
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
            }
        }

        // Drain the per-iteration autorelease pool.
        unsafe { let _: () = msg_send![pool, drain]; }
    }
}

// ── Private helper: rebuild menu and swap it into the tray ───────────────────

fn rebuild_menu(
    tray: &tray_icon::TrayIcon,
    menu_actions: &mut HashMap<tray_icon::menu::MenuId, MenuAction>,
    config: &AppConfig,
) {
    let prompts = load_prompts();
    let (new_menu, new_actions) = build_menu(&prompts, config);
    *menu_actions = new_actions;
    tray.set_menu(Some(Box::new(new_menu)));
}
