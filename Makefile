.PHONY: desktop desktop-test desktop-run clean

CARGO ?= $(HOME)/.cargo/bin/cargo

desktop:
	cd desktop/app && $(CARGO) tauri build

desktop-test:
	cd desktop && $(CARGO) test

desktop-run:
	cd desktop && $(CARGO) run -p syncdrop

clean:
	cd desktop && $(CARGO) clean
	rm -rf .build SyncDrop.app SyncDrop.zip
