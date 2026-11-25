# Qorebit: one-line commands for the QoreChain hardware wallet on the Baochip Dabao (two buttons).
# `make` or `make help` lists everything. Typical first run:
#
#   make install        tools (Rust, Node) + submodule, npm, Python venv, Xous toolchain
#   make demo           emulator + device window + website + local devnet: the whole thing, no hardware
#   make flash-virtual  or, with a Dabao: flash it, its screen is a window here (make board-virtual to reuse)
#   make flash-panel    or flash the build that uses the board's own panel + button (make board-panel)
#
# The device screen opens with the device: `make flash` / `make boot` open it once the board
# runs, `make emu` starts the emulator with it. No board? `make web-emu` is `make web` on the
# emulator. `make test-emu` runs the whole wallet end to end on a fresh emulator and the devnet.
#
# The wallet website has a network switch: mainnet (qorechain-vladi), testnet (qorechain-diana)
# and the local devnet (qorechain-local). All three go through the bridge, so the devnet is
# optional: use the testnet without it.
#
# Device screen: SCREEN=virtual (default for now) approves in a window on this laptop;
# SCREEN=panel builds for the display wired to the board and runs the bridge without a window.
# Pass it to the commands that build firmware or start the bridge, e.g. `make flash SCREEN=panel`.
# The two buttons: left is 0 / reject, right is 1 / approve.

SHELL := /bin/bash
.DEFAULT_GOAL := help

ROOT      := $(abspath .)
FW        := $(ROOT)/firmware/dabao-app-qorebit
XOUS      := $(ROOT)/xous-core
DEVNET    := $(ROOT)/devnet
COMPANION := $(ROOT)/companion
PY        := $(ROOT)/tools/.venv/bin/python

# Boot wait: BOOTWAIT=on (default) makes the board stop in the bootloader on every plug-in, so flashing
# needs no PROG press: unplug, plug in, `make flash`. After flashing, `make boot` starts Qorebit.
# BOOTWAIT=off makes a normal plug-in start Qorebit (hold PROG while plugging in to reach the bootloader);
# BOOTWAIT=keep leaves the setting alone. It is a one-way counter: each change burns one step, so it is
# set once and left alone.
BOOTWAIT ?= on
ifeq ($(BOOTWAIT),keep)
  FLASH_FLAGS := --keep-bootwait
else ifeq ($(BOOTWAIT),off)
  FLASH_FLAGS := --bootwait-off
else ifeq ($(BOOTWAIT),on)
  FLASH_FLAGS :=
else
  $(error BOOTWAIT must be `on` (default), `off` or `keep`, not `$(BOOTWAIT)`)
endif

SCREEN ?= virtual
ifeq ($(SCREEN),virtual)
  VDISPLAY_FLAG := --vdisplay=true
  STACK_FLAGS   :=
else ifeq ($(SCREEN),panel)
  VDISPLAY_FLAG := --vdisplay=false --panel-udma
  STACK_FLAGS   := --panel
else
  $(error SCREEN must be `virtual` (laptop window) or `panel` (display on the board), not `$(SCREEN)`)
endif

.PHONY: help
help: ## list the commands
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_-]+:.*?## / {printf "  \033[1mmake %-17s\033[0m %s\n", $$1, $$2} /^## / {printf "\n\033[2m%s\033[0m\n", substr($$0, 4)}' $(MAKEFILE_LIST)
	@printf "\n\033[2mSCREEN=virtual (default) or SCREEN=panel on firmware, flash, boot, emu, web and bridge commands.\033[0m\n"
	@printf "\033[2mBOOTWAIT=on|off|keep on flash, flash-only and boot: boot1's boot wait (default on: no PROG press needed to flash).\033[0m\n"

## ---- install / setup ---------------------------------------------------

.PHONY: install setup setup-emu submodules deps toolchain doctor
install: ## install the tools (Rust, Node) and everything below (first run)
	$(ROOT)/tools/install.sh
	@# freshly installed tools are not on this shell's PATH yet
	@export PATH="$$HOME/.cargo/bin:$$PATH"; \
	  if ! command -v node >/dev/null && [ -s "$$HOME/.nvm/nvm.sh" ]; then . "$$HOME/.nvm/nvm.sh"; fi; \
	  $(MAKE) --no-print-directory setup && $(MAKE) --no-print-directory doctor

setup: submodules deps toolchain ## project setup once the tools are installed (all: emulator and board)

setup-emu: submodules deps ## setup for the emulator only: no Xous toolchain, so much faster

submodules: ## fetch xous-core (pinned version)
	git submodule update --init xous-core
	@# the image signer versions builds with `git describe --tags`; the btchip fork has no tags
	@git -C $(XOUS) describe --tags >/dev/null 2>&1 || git -C $(XOUS) fetch --quiet --tags https://github.com/betrusted-io/xous-core

deps: ## npm packages (companion, tools) and the Python venv (bridge, client)
	cd $(COMPANION) && npm install
	cd $(ROOT)/tools && npm install
	test -x $(PY) || python3 -m venv $(ROOT)/tools/.venv
	$(PY) -m pip install -q -r $(ROOT)/tools/requirements.txt

toolchain: submodules ## Rust target + Xous toolchain for the Baochip (cargo xtask install-toolkit)
	cd $(XOUS) && cargo xtask install-toolkit

doctor: ## check that the tools this project needs are installed
	@for c in git cargo rustup node npm python3 curl; do \
	  if command -v $$c >/dev/null; then printf "  ok       %s\n" $$c; else printf "  MISSING  %s\n" $$c; fi; done
	@test -d $(XOUS)/.git -o -f $(XOUS)/.git && echo "  ok       xous-core submodule" || echo "  MISSING  xous-core submodule (make submodules)"
	@test -x $(DEVNET)/bin/qorechaind && echo "  ok       devnet/bin/qorechaind" || echo "  missing  devnet/bin/qorechaind (make devnet-bin; only for the local devnet)"
	@test -x $(PY) && echo "  ok       tools/.venv" || echo "  MISSING  tools/.venv (make deps)"
	@test -d $(COMPANION)/node_modules && echo "  ok       companion/node_modules" || echo "  MISSING  companion/node_modules (make deps)"

## ---- local devnet (one QoreChain node on this computer, chain id qorechain-local) --

.PHONY: devnet devnet-bin devnet-up devnet-start devnet-down devnet-info devnet-logs fund
devnet: devnet-bin devnet-up ## fetch the node if needed and start a fresh local devnet

devnet-bin: ## fetch the official QoreChain node release (sha256 checked) into devnet/bin
	$(DEVNET)/get-bin.sh

devnet-up: ## start a fresh chain (wipes the old one), in the background
	$(DEVNET)/devnet.sh up

devnet-start: ## start the existing chain again, keeping its state
	$(DEVNET)/devnet.sh start

devnet-down: ## stop the devnet (its chain data stays until the next make devnet)
	$(DEVNET)/devnet.sh down

devnet-info: ## endpoints, chain id, the faucet account
	$(DEVNET)/devnet.sh info

devnet-logs: ## follow the node's log
	$(DEVNET)/devnet.sh logs

fund: ## send test QOR on the devnet: make fund ADDR=qor1... [AMOUNT=uqor]
	@test -n "$(ADDR)" || { echo "usage: make fund ADDR=qor1... [AMOUNT=1000000000]"; exit 2; }
	node $(ROOT)/tools/fund.mjs $(ADDR) $(or $(AMOUNT),1000000000)

## ---- firmware (Xous app on the Dabao) ---------------------------------

.PHONY: firmware firmware-emu firmware-device flash flash-only boot emu sim emu-only emu-reset screens
firmware: ## build for the emulator and the board
	$(FW)/build.sh all $(VDISPLAY_FLAG)

firmware-emu: ## build the emulator binary
	$(FW)/build.sh emu $(VDISPLAY_FLAG)

firmware-device: ## build the board images (loader.uf2, xous.uf2, apps.uf2)
	$(FW)/build.sh device $(VDISPLAY_FLAG)

flash: flash-only ## build, flash and start the Dabao, then open its screen (just plug the board in)
	@$(ROOT)/tools/stack.sh $(STACK_FLAGS)

flash-only: firmware-device ## build, flash and start the Dabao, without opening the screen
	$(ROOT)/tools/flash.sh $(FLASH_FLAGS)

boot: ## start a board sitting in the bootloader (like pressing PROG), then open its screen
	$(ROOT)/tools/flash.sh --boot $(FLASH_FLAGS)
	@$(ROOT)/tools/stack.sh $(STACK_FLAGS)

emu: firmware-emu ## run Qorebit in the emulator with its screen (device window + bridge; Ctrl+C stops)
	@$(ROOT)/tools/stack.sh --emu $(STACK_FLAGS)

sim: emu ## same as make emu

emu-only: firmware-emu ## the emulator alone, no screen or bridge (device on 127.0.0.1:7610)
	cd $(XOUS) && cargo xtask dabao-emu $(FW)/target/release/qorebit

emu-reset: ## forget the emulator's seed and PIN (its next start is a new device)
	rm -f $(FW)/target/qorebit-state.bin

screens: ## render every device screen to PNGs in screens/ (no device needed)
	mkdir -p $(ROOT)/screens
	cd $(FW) && QOREBIT_SCREENS=$(ROOT)/screens cargo test --release dump_screens -q
	@$(PY) -c "import os,sys; os.environ['PYGAME_HIDE_SUPPORT_PROMPT']='1'; import pygame; d='$(ROOT)/screens'; \
	  [pygame.image.save(pygame.transform.scale(pygame.image.load(os.path.join(d,f)),(480,560)), os.path.join(d,f[:-4]+'.png')) or os.remove(os.path.join(d,f)) for f in os.listdir(d) if f.endswith('.ppm')]"
	@echo "screens in $(ROOT)/screens"

## ---- the four ways to run it (pick one) --------------------------------
# 1. emulator + the device window on this laptop      make demo            (no hardware needed)
# 2. board + the device window on this laptop         make board-virtual   (flash first with make flash-virtual)
# 3. board + its own panel and button, no window      make board-panel     (flash first with make flash-panel)
# 4. checks only, no screens                          make test / make test-emu

.PHONY: demo demo-devnet flash-virtual flash-panel board-virtual board-panel
demo-devnet:
	@if [ -x $(DEVNET)/bin/qorechaind ] || $(DEVNET)/get-bin.sh; then $(DEVNET)/devnet.sh start 2>/dev/null || $(DEVNET)/devnet.sh up; \
	  else echo "no local devnet on this system: use the Testnet tab in the website"; fi

demo: demo-devnet ## EMULATOR + device window + website on http://localhost:5173 (starts the local devnet too); no hardware needed
	$(MAKE) --no-print-directory web-emu

flash-virtual: ## BOARD, virtual display: build, flash and start the Dabao; the screen is a window on this laptop (just plug the board in)
	$(MAKE) --no-print-directory flash SCREEN=virtual

flash-panel: ## BOARD, own display: build for the ST7789 panel + buttons on PB1 / PB2, flash, start; no window (just plug the board in)
	$(MAKE) --no-print-directory flash SCREEN=panel

board-virtual: ## BOARD already flashed with the virtual display: window + website
	$(MAKE) --no-print-directory web SCREEN=virtual

board-panel: ## BOARD already flashed with the panel build: the bridge and website only (the board shows its own screen)
	$(MAKE) --no-print-directory web SCREEN=panel

## ---- companion + bridge (bridge on :7620 for tools, :7630 for the browser) -----

.PHONY: extension web web-emu wallet wallet-emu web-only bridge bridge-emu companion info security account audit harden bio-test
web: ## bridge + device window + wallet website on :5173, for the board (Ctrl+C stops all)
	@$(ROOT)/tools/stack.sh --web $(STACK_FLAGS)

web-emu: firmware-emu ## the same on the emulator: emulator + bridge + window + website
	@$(ROOT)/tools/stack.sh --emu --web $(STACK_FLAGS)

wallet: web ## same as make web
wallet-emu: web-emu ## same as make web-emu

web-only: ## the wallet website alone (when a bridge is already running)
	cd $(COMPANION) && npm run dev

bridge: ## the bridge (+ device window) alone, for the board over USB
	@$(ROOT)/tools/stack.sh $(STACK_FLAGS)

bridge-emu: ## the bridge (+ window) alone, for an emulator started with `make emu-only`
	$(PY) $(ROOT)/tools/qorebit_display.py --emu $(if $(filter panel,$(SCREEN)),--headless,)

extension: ## build the browser extension (side panel) into extension/dist; load it unpacked in Chrome, Brave or Edge
	$(ROOT)/extension/build.sh

companion: ## typecheck and build the website
	cd $(COMPANION) && npm run build

info: ## ask the device (through the bridge) for its version, seed and approval setup
	$(PY) $(ROOT)/tools/qorebit_client.py info

account: ## the device's address and keys: make account N=0
	$(PY) $(ROOT)/tools/qorebit_client.py account $(or $(N),0)

security: ## the chip's security state: developer mode, paranoid mode, alarms, seed storage, PIN, firmware id
	$(PY) $(ROOT)/tools/qorebit_client.py security

audit: ## boot1's audit of a board in its bootloader: boot code vs Baochip's published hashes, lifecycle counters (read-only)
	$(PY) $(ROOT)/tools/audit.py $(if $(RAW),--raw,)

harden: ## IRREVERSIBLE: WHAT=paranoid or WHAT=require-pq on a board in its bootloader (asks you to type a confirmation)
	@test -n "$(WHAT)" || { echo "usage: make harden WHAT=paranoid|require-pq (read tools/harden.py first: it can't be undone)"; exit 2; }
	$(PY) $(ROOT)/tools/harden.py $(WHAT)

bio-test: ## board: check the BIO display and button engines with two BIO test cores (no panel or button needed)
	$(PY) $(ROOT)/tools/qorebit_client.py bio-test

## ---- tests -------------------------------------------------------------

.PHONY: test test-rust test-firmware test-companion test-emu vectors stack
test: test-rust test-firmware test-companion ## run every unit test suite (no devnet needed)

test-rust: ## signer: keys, hybrid signing and transaction reading, byte for byte against QoreSign
	cargo test --release -q

test-firmware: ## firmware unit tests (screens, PIN flows, storage, protocol)
	cd $(FW) && cargo test --release -q

test-companion: ## companion: typecheck, amount parsing, registration message
	cd $(COMPANION) && npm run typecheck && npm test
	node $(ROOT)/tools/check-extension.mjs

test-emu: ## the wallet end to end on a fresh emulator and the local devnet: the chain verifies the device's signatures (needs make devnet)
	$(FW)/build.sh emu --vdisplay=true
	$(ROOT)/tools/emu-test.sh

vectors: ## regenerate crates/qorebit-signer/tests/vectors.json from @qorechain/qoresign
	cd $(ROOT)/tools && node gen-vectors.mjs > $(ROOT)/crates/qorebit-signer/tests/vectors.json

stack: ## how much stack the ML-DSA-87 signing path needs (the firmware gives it 416 KiB)
	@for w in keygen sign pq-public pq-sign register-proof tx-sign; do printf "%-15s " $$w; cargo build --release -q -p qorebit-signer --example stack; \
	  last=none; for k in 512 448 416 384 352 320 288 256 224; do if ./target/release/examples/stack $$k $$w >/dev/null 2>&1; then last=$$k; else break; fi; done; echo "needs at most $$last KiB"; done

## ---- housekeeping --------------------------------------------------------

.PHONY: clean
clean: ## remove build outputs (keeps node_modules, the venv and xous-core's build cache)
	cargo clean
	cd $(FW) && cargo clean
	rm -rf $(COMPANION)/dist $(ROOT)/screens
