#!/bin/sh
# moon installer
#
# Usage (latest):
#   curl -fsSL https://raw.githubusercontent.com/towerforge/moon/main/install.sh | sh
#
# Env overrides:
#   MOON_VERSION=0.2.0         install a specific version
#   MOON_INSTALL_DIR=/opt/bin  install to a custom directory
#   MOON_VARIANT=musl          force the static musl binary on Linux (no glibc dep)
#   MOON_VARIANT=gnu           force the glibc binary on Linux

set -e

REPO="towerforge/moon"
BINARY="moon"
GITHUB_API="https://api.github.com/repos/${REPO}"
GITHUB_RELEASES="https://github.com/${REPO}/releases/download"

# ── colours ──────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
  BOLD='\033[1m';    RESET='\033[0m'
  DIM='\033[2m'
  CYAN='\033[0;36m'
  RED='\033[0;31m';  YELLOW='\033[0;33m'
  B_GREEN='\033[1;32m'; B_BLUE='\033[1;34m'; B_WHITE='\033[1;37m'
else
  BOLD=''; RESET=''; DIM=''
  CYAN=''; RED=''; YELLOW=''
  B_GREEN=''; B_BLUE=''; B_WHITE=''
fi

step()  { printf "\n${BOLD}${CYAN}[%s/%s]${RESET} ${BOLD}%s${RESET}\n" "$1" "$2" "$3"; }
info()  { printf "    ${DIM}%s${RESET}\n"  "$*"; }
ok()    { printf "    ${B_GREEN}✓${RESET}  %s\n" "$*"; }
die()   { printf "\n  ${RED}${BOLD}✗  %s${RESET}\n\n" "$*" >&2; exit 1; }
warn()  { printf "    ${YELLOW}!${RESET}  %s\n" "$*"; }
kv()    { printf "    ${DIM}%-14s${RESET} ${B_WHITE}%s${RESET}\n" "$1" "$2"; }

# ── requirements ─────────────────────────────────────────────────────────────

need() { command -v "$1" >/dev/null 2>&1 || die "Required tool not found: $1"; }
need curl
need tar

# ── platform detection ───────────────────────────────────────────────────────

detect_os() {
  case "$(uname -s)" in
    Linux)  echo linux ;;
    Darwin) echo macos ;;
    MINGW*|MSYS*|CYGWIN*)
            die "On Windows use the PowerShell installer:
  irm https://raw.githubusercontent.com/${REPO}/main/install.ps1 | iex" ;;
    *)      die "Unsupported OS: $(uname -s). moon runs on Linux, macOS and Windows." ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  echo x86_64  ;;
    aarch64|arm64) echo aarch64 ;;
    *)             die "No prebuilt binary for $(uname -m). Build from source: cargo install --git https://github.com/${REPO} moon-cli" ;;
  esac
}

detect_variant() {
  if ldd --version 2>&1 | grep -qi musl; then
    echo musl
  else
    echo gnu
  fi
}

# ── package name ─────────────────────────────────────────────────────────────

build_package_name() {
  _os="$1"; _arch="$2"; _variant="$3"
  if [ "$_os" = "linux" ] && [ "$_variant" = "musl" ]; then
    echo "${BINARY}-${_os}-${_arch}-musl.tar.gz"
  else
    echo "${BINARY}-${_os}-${_arch}.tar.gz"
  fi
}

# ── github helpers ───────────────────────────────────────────────────────────

fetch_latest_version() {
  curl -fsSL "${GITHUB_API}/releases/latest" \
    | grep '"tag_name"' \
    | sed 's/.*"tag_name": *"v\([^"]*\)".*/\1/'
}

# ── checksum verification ────────────────────────────────────────────────────

verify_checksum() {
  _file="$1"; _sums="$2"
  _name=$(basename "$_file")
  _expected=$(grep " ${_name}$" "$_sums" 2>/dev/null | awk '{print $1}')

  if [ -z "$_expected" ]; then
    warn "No checksum entry for ${_name} — skipping"
    return 0
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    _actual=$(sha256sum "$_file" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    _actual=$(shasum -a 256 "$_file" | awk '{print $1}')
  else
    warn "sha256sum / shasum not found — skipping checksum verification"
    return 0
  fi

  [ "$_actual" = "$_expected" ] \
    || die "Checksum mismatch!
  expected: ${_expected}
  got:      ${_actual}"

  ok "SHA-256 verified"
}

# ── install directory ─────────────────────────────────────────────────────────

default_install_dir() {
  if [ "$(id -u)" = "0" ]; then
    echo /usr/local/bin
  else
    echo "${HOME}/.local/bin"
  fi
}

# ─────────────────────────────────────────────────────────────────────────────
# MAIN
# ─────────────────────────────────────────────────────────────────────────────

# Header
printf "\n"
printf "  ${B_BLUE}███╗   ███╗ ██████╗  ██████╗ ███╗   ██╗${RESET}\n"
printf "  ${B_BLUE}████╗ ████║██╔═══██╗██╔═══██╗████╗  ██║${RESET}\n"
printf "  ${B_BLUE}██╔████╔██║██║   ██║██║   ██║██╔██╗ ██║${RESET}\n"
printf "  ${B_BLUE}██║╚██╔╝██║██║   ██║██║   ██║██║╚██╗██║${RESET}\n"
printf "  ${B_BLUE}██║ ╚═╝ ██║╚██████╔╝╚██████╔╝██║ ╚████║${RESET}\n"
printf "  ${B_BLUE}╚═╝     ╚═╝ ╚═════╝  ╚═════╝ ╚═╝  ╚═══╝${RESET}\n"
printf "\n"
printf "  ${DIM}Chat with local language models, from your terminal${RESET}\n"
printf "\n"
printf "  ${DIM}────────────────────────────────────────────${RESET}\n"

# ── step 1: detect platform ──────────────────────────────────────────

step 1 4 "Detecting platform"

OS=$(detect_os)
ARCH=$(detect_arch)

VARIANT=""
if [ "$OS" = "linux" ]; then
  if [ -n "${MOON_VARIANT:-}" ]; then
    VARIANT="$MOON_VARIANT"
    info "Variant overridden via MOON_VARIANT=${VARIANT}"
  else
    VARIANT=$(detect_variant)
  fi
fi

kv "OS:"           "$OS"
kv "Architecture:" "$ARCH"
[ -n "$VARIANT" ] && kv "libc:" "$VARIANT"
ok "Platform detected"

# ── step 2: resolve version ──────────────────────────────────────────────────

step 2 4 "Resolving version"

VERSION="${MOON_VERSION:-}"
if [ -z "$VERSION" ]; then
  info "Querying GitHub releases API..."
  VERSION=$(fetch_latest_version) || die "Could not fetch the latest version from GitHub"
  [ -n "$VERSION" ] || die "No release found at https://github.com/${REPO}/releases"
fi

PACKAGE=$(build_package_name "$OS" "$ARCH" "$VARIANT")
URL="${GITHUB_RELEASES}/v${VERSION}/${PACKAGE}"
CHECKSUMS_URL="${GITHUB_RELEASES}/v${VERSION}/checksums.txt"
INSTALL_DIR="${MOON_INSTALL_DIR:-$(default_install_dir)}"

# Detect an existing installation
CURRENT_VERSION=""
EXISTING_PATH=""
for _candidate in "${INSTALL_DIR}/${BINARY}" "$(command -v ${BINARY} 2>/dev/null || true)"; do
  [ -z "$_candidate" ] && continue
  if [ -x "$_candidate" ]; then
    EXISTING_PATH="$_candidate"
    CURRENT_VERSION=$("$_candidate" --version 2>/dev/null | awk '{print $NF}' || true)
    break
  fi
done

if [ -z "$CURRENT_VERSION" ]; then
  MODE="install"
  kv "Version:"    "v${VERSION}"
  kv "Package:"    "$PACKAGE"
  kv "Install to:" "${INSTALL_DIR}/${BINARY}"
elif [ "$CURRENT_VERSION" = "$VERSION" ]; then
  MODE="reinstall"
  kv "Version:"    "v${VERSION} ${DIM}(already installed at ${EXISTING_PATH})${RESET}"
  kv "Package:"    "$PACKAGE"
  kv "Install to:" "${INSTALL_DIR}/${BINARY}"
else
  MODE="upgrade"
  kv "Version:"    "${YELLOW}v${CURRENT_VERSION}${RESET}  →  ${B_GREEN}v${VERSION}${RESET}"
  kv "Package:"    "$PACKAGE"
  kv "Install to:" "${INSTALL_DIR}/${BINARY}"
fi

ok "Version resolved"

# ── confirmation ─────────────────────────────────────────────────────────────

printf "\n"
if [ -t 0 ] || [ -c /dev/tty ]; then
  if [ "$MODE" = "reinstall" ]; then
    printf "  ${YELLOW}Already at v${VERSION}.${RESET} Reinstall? [y/N] "
    read -r _reply </dev/tty
    case "$_reply" in
      [yY]*) ;;
      *) printf "\n  Aborted.\n\n"; exit 0 ;;
    esac
  else
    _action="install"
    [ "$MODE" = "upgrade" ] && _action="upgrade"
    printf "  Press ${BOLD}Enter${RESET} to ${_action} or ${BOLD}Ctrl+C${RESET} to cancel... "
    read -r _ </dev/tty
  fi
fi

# ── step 3: download & verify ────────────────────────────────────────────────

step 3 4 "Downloading"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

info "URL: ${URL}"
curl -fL --progress-bar -o "${TMP}/${PACKAGE}" "$URL" \
  || die "Download failed. Check that release v${VERSION} has the asset ${PACKAGE}:
  https://github.com/${REPO}/releases/tag/v${VERSION}"

ok "Downloaded ${PACKAGE}"

if curl -fsSL -o "${TMP}/checksums.txt" "$CHECKSUMS_URL" 2>/dev/null; then
  verify_checksum "${TMP}/${PACKAGE}" "${TMP}/checksums.txt"
else
  warn "checksums.txt not available for v${VERSION}"
fi

# ── step 4: install ──────────────────────────────────────────────────────────

step 4 4 "Installing"

info "Extracting archive..."
tar -xzf "${TMP}/${PACKAGE}" -C "$TMP"
[ -f "${TMP}/${BINARY}" ] || die "Binary '${BINARY}' not found inside the archive"

mkdir -p "$INSTALL_DIR"
install -m 755 "${TMP}/${BINARY}" "${INSTALL_DIR}/${BINARY}"

case "$MODE" in
  upgrade)   ok "Upgraded    ${INSTALL_DIR}/${BINARY}  ${DIM}(v${CURRENT_VERSION} → v${VERSION})${RESET}" ;;
  reinstall) ok "Reinstalled ${INSTALL_DIR}/${BINARY}  ${DIM}(v${VERSION})${RESET}" ;;
  *)         ok "Installed   ${INSTALL_DIR}/${BINARY}  ${DIM}(v${VERSION})${RESET}" ;;
esac

# PATH hint
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    printf "\n"
    warn "${INSTALL_DIR} is not in your PATH"
    warn "Add to your shell profile:  ${BOLD}export PATH=\"${INSTALL_DIR}:\$PATH\"${RESET}"
    ;;
esac

# ── done ─────────────────────────────────────────────────────────────────────

printf "\n"
printf "  ${DIM}────────────────────────────────────────────${RESET}\n"
printf "\n"
printf "  ${B_GREEN}${BOLD}All done!${RESET}\n"
printf "\n"
printf "  ${BOLD}Start:${RESET}   ${CYAN}${BINARY}${RESET}\n"
printf "  ${BOLD}Ask:${RESET}     ${CYAN}${BINARY} ask \"explain the borrow checker\"${RESET}\n"
printf "  ${BOLD}Config:${RESET}  ${CYAN}${BINARY} config init${RESET}   ${DIM}(~/.config/moon/config.toml)${RESET}\n"
printf "\n"
printf "  ${DIM}moon talks to Ollama at http://localhost:11434 out of the box.${RESET}\n"
printf "  ${DIM}Have it running with a model pulled:  ollama pull qwen2.5-coder:14b${RESET}\n"
printf "\n"
