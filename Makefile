CARGO ?= cargo
NVIM ?= nvim
PYTHON ?= python3
CARGO_HOME ?= $(CURDIR)/build/cargo
export CARGO_HOME

.PHONY: build test check test-protocol test-neovim
build:
	$(CARGO) build --release --locked
test:
	$(CARGO) test --locked
check:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --all-targets --locked -- -D warnings
test-protocol:
	$(CARGO) build --locked
	$(PYTHON) tests/protocol.py
test-neovim:
	$(CARGO) build --locked
	sh scripts/prepare-editor-test.sh
	XDG_STATE_HOME="$(CURDIR)/.test-work/state" XDG_CACHE_HOME="$(CURDIR)/.test-work/cache" NVIM_LOG_FILE="$(CURDIR)/.test-work/nvim.log" $(NVIM) --headless -u NONE -i NONE -c "lua dofile('tests/neovim.lua')"
