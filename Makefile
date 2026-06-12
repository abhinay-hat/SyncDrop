.PHONY: build app zip install clean test notarize

APP_NAME = SyncDrop
BUILD_DIR = .build/release
APP_BUNDLE = $(APP_NAME).app
CONTENTS = $(APP_BUNDLE)/Contents
ZIP = $(APP_NAME).zip

# Ad-hoc signing identity by default. Override for a real release:
#   make app SIGN_ID="Developer ID Application: Your Name (TEAMID)"
SIGN_ID ?= -
ENTITLEMENTS = Resources/SyncDrop.entitlements

build:
	swift build -c release 2>&1

test:
	swift test 2>&1

app: build
	rm -rf $(APP_BUNDLE)
	mkdir -p $(CONTENTS)/MacOS
	mkdir -p $(CONTENTS)/Resources
	cp $(BUILD_DIR)/$(APP_NAME) $(CONTENTS)/MacOS/
	cp Info.plist $(CONTENTS)/
	cp Resources/AppIcon.icns $(CONTENTS)/Resources/
	# Hardened runtime + entitlements so the exact same bundle can later be
	# notarized without rebuilding. Ad-hoc ("-") until a Developer ID is set.
	codesign --force --deep --options runtime \
		--entitlements $(ENTITLEMENTS) \
		--sign "$(SIGN_ID)" $(APP_BUNDLE)
	codesign --verify --deep --strict $(APP_BUNDLE)
	@echo "✓ Built and signed $(APP_BUNDLE) (identity: $(SIGN_ID))"

# Clean, AppleDouble-free zip for distribution (mirrors codexbar packaging hygiene).
zip: app
	rm -f $(ZIP)
	xattr -cr $(APP_BUNDLE)
	find $(APP_BUNDLE) -name '._*' -delete
	/usr/bin/ditto --norsrc -c -k --keepParent $(APP_BUNDLE) $(ZIP)
	@echo "✓ Packaged $(ZIP)"

install: app
	rm -rf ~/Applications/$(APP_BUNDLE)
	cp -r $(APP_BUNDLE) ~/Applications/
	@echo "✓ Installed to ~/Applications/$(APP_BUNDLE)"

# Sign with Developer ID + notarize + staple. Needs a paid Apple Developer
# account. See Scripts/sign-and-notarize.sh for required env vars.
notarize:
	./Scripts/sign-and-notarize.sh

clean:
	rm -rf .build $(APP_BUNDLE) $(ZIP)
