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

.PHONY: build icns app dmg clean

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
