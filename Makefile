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
RED   := \033[0;31m

VERSION       ?= $(shell cat VERSION)
CARGO_VERSION := $(shell sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -n1)

HOST_OS   := $(shell uname -s | tr '[:upper:]' '[:lower:]')
HOST_ARCH := $(shell uname -m)

# Branches of the release flow: work happens on $(DEV_BRANCH) and a release
# promotes it to $(MAIN_BRANCH), which is what the tag is cut from.
DEV_BRANCH  ?= dev
MAIN_BRANCH ?= main

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
        version set-version write-version release \
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
	@echo "    make release                $(DEV_BRANCH)→$(MAIN_BRANCH): bumps VERSION, merges, tags vX.Y.Z and pushes"
	@echo "                                from $(MAIN_BRANCH): tags the current version and pushes it"
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
#   make release              $(DEV_BRANCH)→$(MAIN_BRANCH): bump, merge, tag and push
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
	$(MAKE) --no-print-directory write-version NEW="$$new"; \
	printf "  $(DIM)updated:$(RESET)\n"; \
	printf "    VERSION      → $$new\n"; \
	printf "    Cargo.toml   → version = \"$$new\"\n"; \
	printf "    Cargo.lock   → synced\n"; \
	printf "\n  $(DIM)review with$(RESET)  git diff  $(DIM)then commit and run$(RESET)  make release\n\n"

# The three files that carry the version, written in one place: `set-version`
# and `release` both go through here.
write-version:
	@printf "%s\n" "$(NEW)" > VERSION
	@awk -v new="$(NEW)" 'BEGIN{done=0} !done && /^version[[:space:]]*=[[:space:]]*"[^"]+"/ {sub(/"[^"]+"/, "\"" new "\""); done=1} {print}' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
	@cargo update --workspace --offline --quiet

## Publishes a release along the fixed chain dev → main: the target branch is
## decided by the branch you are on, without asking.
##
##   · from dev  → main   Proposes the next version (patch/minor/major or one
##                        you type), writes it to VERSION, Cargo.toml and
##                        Cargo.lock, commits it on dev, merges dev into main,
##                        tags vX.Y.Z and pushes. CI builds and publishes.
##   · from main          Promotion: keeps the version, tags it if the tag is
##                        not there yet, and pushes.
##   · another branch     Refused: a release is cut from dev or main.
##
## If the merge collides only in VERSION it is resolved with the release
## version; any other conflict, Cargo.toml included, aborts the merge and
## leaves everything as it was, back on the branch you started from.
release:
	@set -e; \
	if [ -n "$$(git status --porcelain -uno)" ]; then \
		printf "\n  $(RED)✗ Uncommitted changes — commit or stash before releasing.$(RESET)\n\n"; exit 1; \
	fi; \
	if ! git remote get-url origin >/dev/null 2>&1; then \
		printf "\n  $(RED)✗ No 'origin' remote to push to.$(RESET)\n\n"; exit 1; \
	fi; \
	V=$$(cat VERSION 2>/dev/null | tr -d '[:space:]'); \
	if [ "$$V" != "$(CARGO_VERSION)" ]; then \
		printf "\n  $(RED)✗ VERSION ($$V) and Cargo.toml ($(CARGO_VERSION)) differ$(RESET)  $(DIM)run: make set-version $$V$(RESET)\n\n"; exit 1; \
	fi; \
	ORIG=$$(git rev-parse --abbrev-ref HEAD); \
	case "$$ORIG" in \
		$(DEV_BRANCH))  TARGET=$(MAIN_BRANCH); MODE=bump ;; \
		$(MAIN_BRANCH)) TARGET=$(MAIN_BRANCH); MODE=promo ;; \
		*) printf "\n  $(RED)✗ A release is cut from $(DEV_BRANCH) or $(MAIN_BRANCH), not from '$$ORIG'$(RESET)\n\n"; exit 1 ;; \
	esac; \
	if ! git rev-parse -q --verify "refs/heads/$$TARGET" >/dev/null; then \
		if git ls-remote --exit-code --heads origin "$$TARGET" >/dev/null 2>&1; then \
			printf "\n  $(DIM)$$TARGET is not here but it is on origin — creating it...$(RESET)\n"; \
			git fetch -q origin "$$TARGET:$$TARGET"; \
		else \
			printf "\n  $(RED)✗ Branch '$$TARGET' is neither here nor on origin$(RESET)\n\n"; exit 1; \
		fi; \
	fi; \
	CI_MSG="GitHub Actions builds every target, writes checksums.txt and publishes the GitHub release"; \
	if [ "$$MODE" = "promo" ]; then \
		printf "\n  $(BOLD)Release · promotion$(RESET)  $(DIM)$$TARGET · version $$V$(RESET)\n\n"; \
		if git rev-parse -q --verify "refs/tags/v$$V" >/dev/null; then \
			printf "  $(RED)✗ Tag v$$V already exists$(RESET)  $(DIM)bump from $(DEV_BRANCH) instead$(RESET)\n\n"; exit 1; \
		fi; \
		printf "  $(BOLD)Summary$(RESET)\n\n"; \
		printf "    Version  $(BOLD)$$V$(RESET)  $(DIM)kept, no bump$(RESET)\n"; \
		printf "    Tag      v$$V\n"; \
		printf "    Push     origin $$TARGET · origin v$$V\n"; \
		printf "    CI       $(DIM)$$CI_MSG$(RESET)\n"; \
		printf "\n  Continue? [y/N]: "; read ANS; echo ""; \
		if [ "$$ANS" != "y" ]; then \
			printf "  $(DIM)Cancelled — nothing was touched.$(RESET)\n\n"; exit 0; \
		fi; \
		printf "  $(DIM)→ Tagging v$$V$(RESET)\n"; \
		git tag -a "v$$V" -m "Release v$$V"; \
		printf "  $(DIM)→ Publishing $$TARGET and the tag$(RESET)\n"; \
		git push -q origin "$$TARGET"; \
		git push -q origin "v$$V"; \
		printf "\n  $(GREEN)✓ v$$V published from $$TARGET$(RESET)\n\n"; \
		exit 0; \
	fi; \
	IFS=. read MA MI PA <<< "$${V:-0.0.0}"; \
	P_PATCH="$$MA.$$MI.$$((PA+1))"; \
	P_MINOR="$$MA.$$((MI+1)).0"; \
	P_MAJOR="$$((MA+1)).0.0"; \
	printf "\n  $(BOLD)Release$(RESET)  $(DIM)$$ORIG → $$TARGET · current version $$V$(RESET)\n\n"; \
	printf "  $(BOLD)New version$(RESET)\n\n"; \
	printf "    $(BOLD)1$(RESET)  $$P_PATCH  $(DIM)patch$(RESET)\n"; \
	printf "    $(BOLD)2$(RESET)  $$P_MINOR  $(DIM)minor$(RESET)\n"; \
	printf "    $(BOLD)3$(RESET)  $$P_MAJOR  $(DIM)major$(RESET)\n"; \
	printf "    $(DIM)or type the version by hand (X.Y.Z)$(RESET)\n\n"; \
	printf "  > "; read CH; echo ""; \
	case "$$CH" in \
		1) NEW_V=$$P_PATCH ;; \
		2) NEW_V=$$P_MINOR ;; \
		3) NEW_V=$$P_MAJOR ;; \
		*) NEW_V=$$CH ;; \
	esac; \
	if ! [[ "$$NEW_V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$$ ]]; then \
		printf "  $(RED)✗ Invalid version: '$$NEW_V'$(RESET)  $(DIM)expected X.Y.Z, e.g. 0.2.0$(RESET)\n\n"; exit 1; \
	fi; \
	if [ "$$NEW_V" = "$$V" ]; then \
		printf "  $(RED)✗ The new version is the current one ($$V)$(RESET)\n\n"; exit 1; \
	fi; \
	if git rev-parse -q --verify "refs/tags/v$$NEW_V" >/dev/null; then \
		printf "  $(RED)✗ Tag v$$NEW_V already exists$(RESET)\n\n"; exit 1; \
	fi; \
	printf "  $(BOLD)Summary$(RESET)\n\n"; \
	printf "    Version  $(DIM)$$V →$(RESET) $(BOLD)$$NEW_V$(RESET)  $(DIM)VERSION · Cargo.toml · Cargo.lock$(RESET)\n"; \
	printf "    Merge    $$ORIG → $$TARGET\n"; \
	printf "    Tag      v$$NEW_V\n"; \
	printf "    Push     origin $$ORIG · origin $$TARGET · origin v$$NEW_V\n"; \
	printf "    CI       $(DIM)$$CI_MSG$(RESET)\n"; \
	printf "\n  Continue? [y/N]: "; read ANS; echo ""; \
	if [ "$$ANS" != "y" ]; then \
		printf "  $(DIM)Cancelled — nothing was touched.$(RESET)\n\n"; exit 0; \
	fi; \
	printf "  $(DIM)→ VERSION $$V → $$NEW_V · commit and push on $$ORIG$(RESET)\n"; \
	$(MAKE) --no-print-directory write-version NEW="$$NEW_V"; \
	git add VERSION Cargo.toml Cargo.lock; \
	git commit -q -m "chore: bump version to $$NEW_V"; \
	git push -q origin "$$ORIG"; \
	printf "  $(DIM)→ Merging $$ORIG into $$TARGET$(RESET)\n"; \
	git checkout -q "$$TARGET"; \
	if git ls-remote --exit-code --heads origin "$$TARGET" >/dev/null 2>&1; then \
		git pull -q --ff-only origin "$$TARGET"; \
	fi; \
	if ! git merge -q --no-ff "$$ORIG" -m "release: v$$NEW_V"; then \
		CONFLICTS=$$(git diff --name-only --diff-filter=U); \
		if [ "$$CONFLICTS" = "VERSION" ]; then \
			printf "  $(DIM)→ Conflict in VERSION — resolved with $$NEW_V$(RESET)\n"; \
			printf "%s\n" "$$NEW_V" > VERSION; \
			git add VERSION; \
			git commit -q --no-edit; \
		else \
			printf "\n  $(RED)✗ Merge conflicts in:$(RESET)\n"; \
			echo "$$CONFLICTS" | sed 's/^/      /'; \
			git merge --abort; \
			git checkout -q "$$ORIG"; \
			printf "\n  $(DIM)Merge aborted — the bump is committed on $$ORIG but nothing was published. Back on $$ORIG.$(RESET)\n\n"; \
			exit 1; \
		fi; \
	fi; \
	printf "  $(DIM)→ Tagging v$$NEW_V$(RESET)\n"; \
	git tag -a "v$$NEW_V" -m "Release v$$NEW_V"; \
	printf "  $(DIM)→ Publishing $$TARGET and the tag$(RESET)\n"; \
	git push -q origin "$$TARGET"; \
	git push -q origin "v$$NEW_V"; \
	git checkout -q "$$ORIG"; \
	echo ""; \
	printf "  $(GREEN)✓ Release v$$NEW_V published on $$TARGET$(RESET)\n"; \
	printf "\n  Back on $(BOLD)$$ORIG$(RESET)\n\n"

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
