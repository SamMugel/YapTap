# Packaging

YapTap ships as a macOS `.app` bundle distributed inside a `.dmg` disk image. The build pipeline is driven by a `Makefile` at the project root.

---

## App Bundle Structure

```
YapTap.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/
    │   └── yaptap                       # compiled Rust binary (release mode)
    └── Resources/
        ├── config/
        │   └── prompts/                 # bundled TOML prompt files (from config/prompts/)
        ├── icons/                       # menu bar PNG icons (from assets/icons/)
        │   ├── yaptap-idle.png
        │   ├── yaptap-idle@2x.png
        │   ├── yaptap-active.png
        │   └── yaptap-active@2x.png
        ├── scripts/                     # Python helper scripts (from src/core/)
        │   ├── transcribe.py
        │   └── llm.py
        └── YapTap.icns                  # app icon (generated from menu bar idle icon)
```

The `dist/` directory is git-ignored. The bundle is assembled fresh on every `make app` invocation.

---

## Info.plist

Location: `assets/Info.plist` (source-controlled; copied into bundle by `make app`).

Required keys:

| Key | Value |
|-----|-------|
| `CFBundleIdentifier` | `com.yaptap.app` |
| `CFBundleName` | `YapTap` |
| `CFBundleDisplayName` | `YapTap` |
| `CFBundleVersion` | `0.3.0` |
| `CFBundleShortVersionString` | `0.3` |
| `CFBundleIconFile` | `YapTap` |
| `CFBundleExecutable` | `yaptap` |
| `CFBundlePackageType` | `APPL` |
| `LSUIElement` | `true` — menu bar–only app; no Dock icon |
| `LSMinimumSystemVersion` | `12.0` |
| `NSHighResolutionCapable` | `true` |
| `NSMicrophoneUsageDescription` | `"YapTap records audio to transcribe your speech."` |
| `NSPrincipalClass` | `NSApplication` |

---

## App Icon (YapTap.icns)

The app icon is generated from the existing menu bar idle icon at `assets/icons/yaptap-idle@2x.png` using `sips` and `iconutil` (both built into macOS — no extra tools required).

> **Note:** The source image is 36×36 px. Scaled to larger sizes it will appear pixelated; this is acceptable for private/personal use. Replace `assets/icons/yaptap-idle@2x.png` with a higher-resolution source image to improve quality in the future.

### Iconset sizes

`make icns` generates `assets/icons/AppIcon.iconset/` containing:

| Filename | Pixel size |
|----------|-----------|
| `icon_16x16.png` | 16×16 |
| `icon_16x16@2x.png` | 32×32 |
| `icon_32x32.png` | 32×32 |
| `icon_32x32@2x.png` | 64×64 |
| `icon_128x128.png` | 128×128 |
| `icon_128x128@2x.png` | 256×256 |
| `icon_256x256.png` | 256×256 |
| `icon_256x256@2x.png` | 512×512 |
| `icon_512x512.png` | 512×512 |
| `icon_512x512@2x.png` | 1024×1024 |

`iconutil -c icns` converts the iconset to `assets/icons/YapTap.icns`.

Both `assets/icons/AppIcon.iconset/` and `assets/icons/YapTap.icns` are git-ignored (generated artifacts).

---

## Resource Path Resolution

The Rust binary locates bundled resources relative to its own executable path. This works both inside the `.app` bundle and during development (`cargo run`).

```rust
fn resources_dir() -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    // In bundle: .../YapTap.app/Contents/MacOS/yaptap
    //   → resolve to .../YapTap.app/Contents/Resources/
    // In dev:    .../target/release/yaptap
    //   → fall back to project root (relative to exe or CWD)
    if let Some(macos_dir) = exe.parent() {
        let candidate = macos_dir.join("../Resources");
        if candidate.exists() {
            return Ok(candidate.canonicalize()?);
        }
    }
    // Development fallback: resolve from current working directory
    Ok(std::env::current_dir()?)
}
```

- **Prompts:** `<resources>/config/prompts/`
- **Scripts:** `<resources>/scripts/` (used instead of `src/core/` when running from bundle)
- **Icons:** `<resources>/icons/`

The binary resolves `scripts/transcribe.py` and `scripts/llm.py` via `resources_dir()` at startup, before spawning any subprocess.

---

## First-Launch Python Setup

On launch, the binary checks whether a virtual environment exists at `~/.config/yaptap/.venv/`.

### Detection

Check for `~/.config/yaptap/.venv/bin/python`. If it exists, setup is skipped.

### Setup Sequence

If the venv is absent, the binary:

1. Shows a blocking `NSAlert` before completing startup:
   > **"Setting up YapTap…"**
   > First-launch setup is installing Python dependencies. This takes about 30 seconds.
   > *(no buttons — alert auto-dismisses when setup completes)*

   Because `LSUIElement = true`, the alert must be shown by first calling `NSApp.activateIgnoringOtherApps(true)` to bring the app to front.

2. Runs the following commands in a background thread (sequentially):
   ```sh
   python3 -m venv ~/.config/yaptap/.venv
   ~/.config/yaptap/.venv/bin/pip install --quiet "numpy<2" openai-whisper ollama openai tomli
   ```

3. Checks `which ffmpeg` after pip install completes. If ffmpeg is not on PATH, shows a separate one-time alert:
   > **"ffmpeg not found"**
   > YapTap requires ffmpeg for audio processing. Install it with:
   > `brew install ffmpeg`
   > *(button: OK)*

4. Dismisses the setup alert and continues normal startup.

### Venv Health Check and Repair

On every launch (not just first launch), the binary calls `venv_healthy()` before spawning any Python subprocess. `venv_healthy()` returns `true` if:

1. `~/.config/yaptap/.venv/bin/python` exists, AND
2. The venv's `pip` can resolve `openai-whisper`, `ollama`, `openai`, and `tomli` (fast import check, not a network call).

If the venv exists but `venv_healthy()` returns `false` (e.g. after a macOS upgrade broke the interpreter), a two-stage repair is attempted:

1. **In-place repair:** run `pip install --quiet "numpy<2" openai-whisper ollama openai tomli` inside the existing venv. If this succeeds and `venv_healthy()` passes, continue normally.
2. **Full teardown:** if pip repair fails, delete `~/.config/yaptap/.venv/` entirely and re-run the full setup sequence (venv create + full pip install). Shows the same "Setting up YapTap…" alert during repair.

This avoids unnecessary full re-installs for common breakage patterns (missing packages) while still recovering from corrupt interpreter states.

### Setup Failure

If `python3 -m venv` or `pip install` exits non-zero:

- Show alert:
  > **"Setup failed"**
  > YapTap could not install Python dependencies. Ensure `python3` is installed and try launching again.
  > *(button: OK)*
- Do **not** write a sentinel file — absence of `.venv` triggers a retry on next launch.
- Continue startup; transcription and LLM features will be unavailable until setup succeeds.

### Python Interpreter for Subprocesses

After setup, the binary uses the venv interpreter for all subprocess calls:

1. At startup, resolve the interpreter path:
   - If `~/.config/yaptap/.venv/bin/python` exists → use it
   - Otherwise → fall back to `python3` on PATH
2. Store the resolved path and use it whenever spawning `transcribe.py` or `llm.py`.

---

## Whisper Model Download

Whisper downloads model weights to `~/.cache/whisper/` automatically on the first call to `whisper.load_model()` inside `transcribe.py`. No explicit download step is needed in the setup sequence.

### User-visible behaviour

- First recording attempt may take 1–3 minutes while the `base` model (~145 MB) downloads.
- The menu bar icon remains in **Active** state throughout.
- `transcribe.py` writes a single log line to stderr before loading: `"Downloading Whisper base model (~145 MB) — first use only…"`. The Rust binary captures and ignores this (stderr is not displayed in app mode).
- Subsequent recordings are fast (model is cached).

---

## Ollama Availability Check

Before invoking `llm.py`, the Rust binary probes Ollama with a TCP connection to `127.0.0.1:11434` (connection-only, no data sent, 1-second timeout).

If the probe fails:

- Show alert:
  > **"Ollama not running"**
  > Start Ollama and try again. Run `ollama serve` in a terminal, or open the Ollama app.
  > *(button: OK)*
- Transition state machine back to **IDLE**.
- Do **not** attempt to launch or manage Ollama.

This check runs only when a prompt is selected (i.e., the LLM step is needed). "No Prompt" mode skips the Ollama check entirely.

---

## Makefile

Location: `Makefile` at project root (source-controlled).

```makefile
# Directories
DIST        = dist
APP_DIR     = $(DIST)/YapTap.app
CONTENTS    = $(APP_DIR)/Contents
STAGING_DIR = $(DIST)/dmg-staging
DMG_PATH    = $(DIST)/YapTap.dmg

# Icon source
SOURCE_ICON = assets/icons/yaptap-idle@2x.png
ICONSET     = assets/icons/AppIcon.iconset
ICNS        = assets/icons/YapTap.icns

.PHONY: build icns app install dmg clean

## Compile Rust binary in release mode
build:
	cargo build --release

## Generate YapTap.icns from the menu bar idle icon
icns:
	mkdir -p $(ICONSET)
	sips -z 16   16   $(SOURCE_ICON) --out $(ICONSET)/icon_16x16.png
	sips -z 32   32   $(SOURCE_ICON) --out $(ICONSET)/icon_16x16@2x.png
	sips -z 32   32   $(SOURCE_ICON) --out $(ICONSET)/icon_32x32.png
	sips -z 64   64   $(SOURCE_ICON) --out $(ICONSET)/icon_32x32@2x.png
	sips -z 128  128  $(SOURCE_ICON) --out $(ICONSET)/icon_128x128.png
	sips -z 256  256  $(SOURCE_ICON) --out $(ICONSET)/icon_128x128@2x.png
	sips -z 256  256  $(SOURCE_ICON) --out $(ICONSET)/icon_256x256.png
	sips -z 512  512  $(SOURCE_ICON) --out $(ICONSET)/icon_256x256@2x.png
	sips -z 512  512  $(SOURCE_ICON) --out $(ICONSET)/icon_512x512.png
	sips -z 1024 1024 $(SOURCE_ICON) --out $(ICONSET)/icon_512x512@2x.png
	iconutil -c icns $(ICONSET) -o $(ICNS)

## Assemble YapTap.app bundle
app: build icns
	rm -rf $(APP_DIR)
	mkdir -p $(CONTENTS)/MacOS \
	         $(CONTENTS)/Resources/config/prompts \
	         $(CONTENTS)/Resources/icons \
	         $(CONTENTS)/Resources/scripts

	# Binary
	cp target/release/yaptap $(CONTENTS)/MacOS/yaptap

	# Plist
	cp assets/Info.plist $(CONTENTS)/Info.plist

	# Prompts
	cp config/prompts/*.toml $(CONTENTS)/Resources/config/prompts/

	# Menu bar icons
	cp assets/icons/yaptap-idle.png       $(CONTENTS)/Resources/icons/
	cp assets/icons/yaptap-idle@2x.png    $(CONTENTS)/Resources/icons/
	cp assets/icons/yaptap-active.png     $(CONTENTS)/Resources/icons/
	cp assets/icons/yaptap-active@2x.png  $(CONTENTS)/Resources/icons/

	# Python scripts
	cp src/core/transcribe.py $(CONTENTS)/Resources/scripts/
	cp src/core/llm.py        $(CONTENTS)/Resources/scripts/

	# App icon
	cp $(ICNS) $(CONTENTS)/Resources/YapTap.icns

## Install app bundle to /Applications/ (replaces existing copy)
install: app
	rm -rf /Applications/YapTap.app
	cp -r $(APP_DIR) /Applications/YapTap.app
	@echo "Installed: /Applications/YapTap.app"

## Build distributable DMG
dmg: app
	rm -rf $(STAGING_DIR) $(DMG_PATH)
	mkdir -p $(STAGING_DIR)

	cp -r $(APP_DIR) $(STAGING_DIR)/
	ln -s /Applications $(STAGING_DIR)/Applications

	hdiutil create \
		-volname "YapTap" \
		-srcfolder $(STAGING_DIR) \
		-ov \
		-format UDZO \
		$(DMG_PATH)

	rm -rf $(STAGING_DIR)
	@echo "Built: $(DMG_PATH)"

## Remove all build artifacts
clean:
	cargo clean
	rm -rf $(DIST) $(ICONSET) $(ICNS)
```

### Target summary

| Target | Depends on | Output |
|--------|-----------|--------|
| `make build` | — | `target/release/yaptap` |
| `make icns` | — | `assets/icons/YapTap.icns` |
| `make app` | `build`, `icns` | `dist/YapTap.app` |
| `make install` | `app` | `/Applications/YapTap.app` |
| `make dmg` | `app` | `dist/YapTap.dmg` |
| `make clean` | — | removes `dist/`, iconset, `.icns` |

---

## New Files

| File | Purpose | Tracked in git |
|------|---------|----------------|
| `Makefile` | Build automation | Yes |
| `assets/Info.plist` | macOS bundle metadata | Yes |
| `dist/YapTap.dmg` | Distributable disk image | No (git-ignored) |
| `dist/YapTap.app` | App bundle | No (git-ignored) |
| `assets/icons/AppIcon.iconset/` | Intermediate iconset directory | No (git-ignored) |
| `assets/icons/YapTap.icns` | Generated app icon | No (git-ignored) |

### .gitignore additions

```
dist/
assets/icons/AppIcon.iconset/
assets/icons/YapTap.icns
```

---

## Installation (end-user steps)

1. Open `YapTap.dmg`.
2. Drag `YapTap` to the `Applications` shortcut in the DMG window.
3. Eject the DMG.
4. Open `YapTap` from `/Applications`.
5. On first launch: wait ~30 seconds for the Python setup alert to dismiss automatically.
6. Grant **Accessibility** permission when prompted (required for the global hotkey).
7. If prompted about `ffmpeg`: run `brew install ffmpeg` in a terminal.
8. Ensure Ollama is running before using prompts: `ollama serve` (or open the Ollama app).
