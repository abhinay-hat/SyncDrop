.PHONY: build app install clean test

APP_NAME = SyncDrop
BUILD_DIR = .build/release
APP_BUNDLE = $(APP_NAME).app
CONTENTS = $(APP_BUNDLE)/Contents

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
	codesign --force --deep --sign - $(APP_BUNDLE)
	@echo "✓ Built and signed $(APP_BUNDLE)"

install: app
	rm -rf ~/Applications/$(APP_BUNDLE)
	cp -r $(APP_BUNDLE) ~/Applications/
	@echo "✓ Installed to ~/Applications/$(APP_BUNDLE)"

clean:
	rm -rf .build $(APP_BUNDLE)
