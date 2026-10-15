.PHONY: desktop desktop-test desktop-run clean release

CARGO ?= $(HOME)/.cargo/bin/cargo
VERSION := $(shell awk -F= '/^MARKETING_VERSION=/{print $$2}' version.env)

desktop:
	cd desktop/app && $(CARGO) tauri build

desktop-test:
	cd desktop && $(CARGO) test

desktop-run:
	cd desktop && $(CARGO) run -p syncdrop

clean:
	cd desktop && $(CARGO) clean
	rm -rf .build SyncDrop.app SyncDrop.zip

release:
	@echo "git tag v$(VERSION) && git push origin v$(VERSION)"
