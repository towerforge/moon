# moon — entry point. Builds and runs on the host: it is a TUI, it does not go
# in Docker. The one exception is cross-compiling for Linux and Windows, which
# `cross` does inside a Docker container.
SHELL := /bin/bash
.DEFAULT_GOAL := help

# ---------------------------------------------------------------------------
# Project
# ---------------------------------------------------------------------------

APP_NAME  := moon
CLI_CRATE := moon-cli
DIST_DIR  ?= dist

RESET := \033[0m
BOLD  := \033[1m
DIM   := \033[2m
GREEN := \033[0;32m

VERSION       ?= $(shell cat VERSION)
CARGO_VERSION := $(shell sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -n1)

HOST_OS   := $(shell uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH := $(shell uname -m)

# ---------------------------------------------------------------------------
# Build target — set CARGO_TARGET to a Rust triple to cross-compile.
#   Linux and Windows targets build inside Docker with `cross` (CARGO_CMD=cross).
#   macOS targets build natively on a Mac (rustup target add <triple>).
#   Install cross once: cargo install cross --git https://github.com/cross-rs/cross
# ---------------------------------------------------------------------------

CARGO_TARGET ?=
CARGO_CMD    ?= cargo

PLATFORM ?= $(strip $(if $(CARGO_TARGET),\
	$(if $(findstring apple-darwin,$(CARGO_TARGET)),macos,\
	$(if $(findstring windows,$(CARGO_TARGET)),windows,linux)),\
	$(if $(filter darwin,$(HOST_OS)),macos,linux)))

BIN_EXT := $(if $(filter windows,$(PLATFORM)),.exe,)
BIN     := $(if $(CARGO_TARGET),target/$(CARGO_TARGET)/release/$(APP_NAME)$(BIN_EXT),target/release/$(APP_NAME))

# Artifacts use the Rust arch name: uname says arm64 on Apple Silicon, Rust says aarch64
TARGET_ARCH   := $(subst arm64,aarch64,$(if $(CARGO_TARGET),$(word 1,$(subst -, ,$(CARGO_TARGET))),$(HOST_ARCH)))
ARTIFACT_ARCH ?= $(TARGET_ARCH)$(if $(findstring musl,$(CARGO_TARGET)),-musl,)

# e.g. moon-linux-x86_64.tar.gz · moon-linux-aarch64-musl.tar.gz · moon-macos-aarch64.tar.gz · moon-windows-x86_64.zip
ARCHIVE_EXT := $(if $(filter windows,$(PLATFORM)),.zip,.tar.gz)
ARTIFACT    := $(APP_NAME)-$(PLATFORM)-$(ARTIFACT_ARCH)$(ARCHIVE_EXT)

LINUX_TARGETS ?= \
	x86_64-unknown-linux-gnu \
	x86_64-unknown-linux-musl \
	aarch64-unknown-linux-gnu \
	aarch64-unknown-linux-musl

MACOS_TARGETS ?= \
	x86_64-apple-darwin \
	aarch64-apple-darwin

WINDOWS_TARGETS ?= \
	x86_64-pc-windows-gnu

# ---------------------------------------------------------------------------
# Phony targets
# ---------------------------------------------------------------------------

.PHONY: help check fmt fmt-check clippy test build run install \
        version set-version release \
        package package-one package-all \
        package-all-linux package-all-macos package-all-windows \
        checksums rust-targets clean dist-clean

# ---------------------------------------------------------------------------
# Help
# ---------------------------------------------------------------------------

help:
	@printf "\n  $(BOLD)$(APP_NAME)$(RESET)  $(DIM)v$(VERSION)$(RESET)\n\n"
	@echo "  Development"
	@echo "    make check                  fmt --check + clippy -D warnings + tests (what CI runs)"
	@echo "    make fmt                    Format the workspace"
	@echo "    make run ARGS=\"ask hi\"      Run from source"
	@echo "    make build                  Release binary for the host (or CARGO_TARGET)"
	@echo "    make install                cargo install from this checkout"
	@echo ""
	@echo "  Packaging"
	@echo "    make package                Build + package for the host (or CARGO_TARGET) into $(DIST_DIR)/"
	@echo "    make package-all            Every Linux + macOS + Windows target"
	@echo "    make package-all-linux      Linux targets (cross + Docker)"
	@echo "    make package-all-macos      macOS targets (needs a Mac)"
	@echo "    make package-all-windows    Windows targets (cross + Docker)"
	@echo "    make checksums              $(DIST_DIR)/checksums.txt (SHA-256)"
	@echo ""
	@echo "  Release"
	@echo "    make version                Show the version and flag drift between VERSION and Cargo.toml"
	@echo "    make set-version x.y.z      Write a new version to VERSION, Cargo.toml and Cargo.lock"
	@echo "    make release                Tag v\$$VERSION and push it; CI builds and publishes the release"
	@echo ""
	@echo "  Utilities"
	@echo "    make rust-targets           List the targets package-all builds"
	@echo "    make clean                  cargo clean"
	@echo "    make dist-clean             Remove $(DIST_DIR)/"
	@echo ""
	@echo "  Variables"
	@echo "    CARGO_TARGET=<triple>       Rust target triple (e.g. aarch64-unknown-linux-musl)"
	@echo "    CARGO_CMD=cross             Build with cross instead of cargo"
	@echo "    DIST_DIR=<dir>              Output directory for artifacts (default: dist)"
	@echo "    LINUX_TARGETS=\"...\"        Triples for package-all-linux"
	@echo "    MACOS_TARGETS=\"...\"        Triples for package-all-macos"
	@echo "    WINDOWS_TARGETS=\"...\"      Triples for package-all-windows"
	@echo ""

# ---------------------------------------------------------------------------
# Development
# ---------------------------------------------------------------------------

check: fmt-check clippy test        ## lint + tests (yes, tests are included here)

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

build:
	$(CARGO_CMD) build --release --locked -p $(CLI_CRATE) $(if $(CARGO_TARGET),--target $(CARGO_TARGET),)

run:
	cargo run -p $(CLI_CRATE) -- $(ARGS)

install:
	cargo install --path crates/cli --locked

# ---------------------------------------------------------------------------
# Version
#   make version              Show the version and flag drift between VERSION
#                             and Cargo.toml
#   make set-version x.y.z    Write a new semver version to VERSION, Cargo.toml
#                             and Cargo.lock (also accepts NEW=x.y.z)
#   make release              Tag v$VERSION and push the tag; CI does the rest
# ---------------------------------------------------------------------------

# Accept a bare positional argument:  make set-version 0.2.0
# Without this Make would treat "0.2.0" as another target.
ifeq (set-version,$(firstword $(MAKECMDGOALS)))
  SET_VERSION_POS := $(wordlist 2,$(words $(MAKECMDGOALS)),$(MAKECMDGOALS))
  ifneq ($(SET_VERSION_POS),)
    $(eval $(SET_VERSION_POS):;@:)
    NEW ?= $(firstword $(SET_VERSION_POS))
  endif
endif

version:
	@printf "\n  $(BOLD)$(APP_NAME)$(RESET)  $(GREEN)v$(VERSION)$(RESET)\n"
	@printf "  $(DIM)VERSION      → $(VERSION)$(RESET)\n"
	@printf "  $(DIM)Cargo.toml   → $(CARGO_VERSION)$(RESET)\n"
	@if [ "$(VERSION)" != "$(CARGO_VERSION)" ]; then \
		printf "\n  $(BOLD)⚠ drift$(RESET)  $(DIM)Cargo.toml is out of sync — run:$(RESET)  make set-version $(VERSION)\n"; \
	fi
	@printf "\n  $(DIM)bump:$(RESET)  make set-version x.y.z\n\n"

set-version:
	@if [ -z "$(NEW)" ]; then \
		printf "\n  $(BOLD)error$(RESET)  missing version  $(DIM)(usage: make set-version 0.2.0)$(RESET)\n\n"; exit 1; \
	fi
	@if ! echo "$(NEW)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$'; then \
		printf "\n  $(BOLD)error$(RESET)  '$(NEW)' is not semver (x.y.z)\n\n"; exit 1; \
	fi
	@old="$(VERSION)"; new="$(NEW)"; \
	if [ "$$old" = "$$new" ] && [ "$(CARGO_VERSION)" = "$$new" ]; then \
		printf "\n  $(DIM)already at v$$new — nothing to do$(RESET)\n\n"; exit 0; \
	fi; \
	printf "\n  $(BOLD)bump$(RESET)  $(DIM)v$$old$(RESET)  →  $(GREEN)v$$new$(RESET)\n\n"; \
	printf "%s\n" "$$new" > VERSION; \
	awk -v new="$$new" 'BEGIN{done=0} !done && /^version[[:space:]]*=[[:space:]]*"[^"]+"/ {sub(/"[^"]+"/, "\"" new "\""); done=1} {print}' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml; \
	cargo update --workspace --offline --quiet; \
	printf "  $(DIM)updated:$(RESET)\n"; \
	printf "    VERSION      → $$new\n"; \
	printf "    Cargo.toml   → version = \"$$new\"\n"; \
	printf "    Cargo.lock   → synced\n"; \
	printf "\n  $(DIM)review with$(RESET)  git diff  $(DIM)then commit and run$(RESET)  make release\n\n"

release:
	@v="$(VERSION)"; \
	if [ "$$v" != "$(CARGO_VERSION)" ]; then \
		printf "\n  $(BOLD)error$(RESET)  VERSION ($$v) and Cargo.toml ($(CARGO_VERSION)) differ — run:  make set-version $$v\n\n"; exit 1; \
	fi; \
	if [ -n "$$(git status --porcelain)" ]; then \
		printf "\n  $(BOLD)error$(RESET)  working tree is not clean — commit first\n\n"; exit 1; \
	fi; \
	if git rev-parse -q --verify "refs/tags/v$$v" >/dev/null; then \
		printf "\n  $(BOLD)error$(RESET)  tag v$$v already exists\n\n"; exit 1; \
	fi; \
	git tag -a "v$$v" -m "release: v$$v" && git push origin "v$$v"; \
	printf "\n  $(GREEN)v$$v$(RESET)  $(DIM)pushed — CI builds the binaries and publishes the GitHub release$(RESET)\n\n"

# ---------------------------------------------------------------------------
# Packaging — dist/moon-<platform>-<arch>.tar.gz (.zip on Windows), one binary inside
# ---------------------------------------------------------------------------

package: build
	@$(MAKE) --no-print-directory package-one

package-one:
	@if [ ! -f "$(BIN)" ]; then \
		echo "ERROR: binary not found at $(BIN) (run make build first)"; exit 1; \
	fi
	@mkdir -p "$(DIST_DIR)"
	@rm -f "$(DIST_DIR)/$(ARTIFACT)"
	@if [ "$(PLATFORM)" = "windows" ]; then \
		cp "$(BIN)" "$(DIST_DIR)/$(APP_NAME).exe"; \
		( cd "$(DIST_DIR)" && zip -q "$(ARTIFACT)" "$(APP_NAME).exe" ); \
		rm -f "$(DIST_DIR)/$(APP_NAME).exe"; \
	else \
		cp "$(BIN)" "$(DIST_DIR)/$(APP_NAME)" && chmod +x "$(DIST_DIR)/$(APP_NAME)"; \
		COPYFILE_DISABLE=1 tar -czf "$(DIST_DIR)/$(ARTIFACT)" -C "$(DIST_DIR)" "$(APP_NAME)"; \
		rm -f "$(DIST_DIR)/$(APP_NAME)"; \
	fi
	@echo "$(DIST_DIR)/$(ARTIFACT)"

package-all: package-all-linux package-all-macos package-all-windows

package-all-linux:
	@set -e; \
	for target in $(LINUX_TARGETS); do \
		echo "==> [linux] $$target (cross + Docker)"; \
		$(MAKE) --no-print-directory package CARGO_TARGET=$$target CARGO_CMD=cross; \
	done

package-all-macos:
	@if [ "$(HOST_OS)" != "darwin" ]; then \
		echo "ERROR: macOS targets can only be built on a Mac"; exit 1; \
	fi
	@set -e; \
	for target in $(MACOS_TARGETS); do \
		echo "==> [macos] $$target"; \
		rustup target add $$target >/dev/null; \
		$(MAKE) --no-print-directory package CARGO_TARGET=$$target; \
	done

package-all-windows:
	@set -e; \
	for target in $(WINDOWS_TARGETS); do \
		echo "==> [windows] $$target (cross + Docker)"; \
		$(MAKE) --no-print-directory package CARGO_TARGET=$$target CARGO_CMD=cross; \
	done

# ---------------------------------------------------------------------------
# Utilities
# ---------------------------------------------------------------------------

checksums:
	@cd "$(DIST_DIR)" 2>/dev/null || { echo "ERROR: $(DIST_DIR) does not exist"; exit 1; }; \
	shopt -s nullglob; files=$$(echo *.tar.gz *.zip); \
	if [ -z "$$files" ]; then echo "ERROR: no .tar.gz or .zip files found in $(DIST_DIR)"; exit 1; fi; \
	{ if command -v sha256sum >/dev/null 2>&1; then sha256sum $$files; else shasum -a 256 $$files; fi; } > checksums.txt; \
	echo "$(DIST_DIR)/checksums.txt"

rust-targets:
	@for target in $(LINUX_TARGETS); do echo $$target; done
	@for target in $(MACOS_TARGETS); do echo $$target; done
	@for target in $(WINDOWS_TARGETS); do echo $$target; done

clean:
	cargo clean

dist-clean:
	@if [ -z "$(DIST_DIR)" ] || [ "$(DIST_DIR)" = "/" ] || [ "$(DIST_DIR)" = "." ]; then \
		echo "ERROR: refusing to remove DIST_DIR='$(DIST_DIR)'"; exit 1; \
	fi
	rm -rf "$(DIST_DIR)"
