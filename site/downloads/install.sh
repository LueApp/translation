#!/usr/bin/env bash
# Install AI Translate from the prebuilt tarball.
#
# Installs the `ai-translate` binary into ~/.local/bin, registers a systemd
# *user* service for the global-hotkey daemon, and adds an app-menu entry with
# "Translate Selection" / "Capture & Translate (OCR)" actions.
#
# Idempotent: safe to re-run. Nothing here needs sudo — everything lands in
# your home directory. (The optional runtime tools below are installed with apt
# and do need sudo; the script only prints the command, it never runs it.)
#
#   ./install.sh                 # install + enable + start the daemon
#   ./install.sh --no-service    # install the binary + .desktop only
#   ./install.sh --uninstall     # remove everything this script installed
#   -h | --help

set -euo pipefail

APP_ID="app-io.github.lue.AiTranslate"
BIN_NAME="ai-translate"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
UNIT_DIR="$HOME/.config/systemd/user"
DESKTOP_DIR="$HOME/.local/share/applications"
UNIT_PATH="$UNIT_DIR/$APP_ID.service"
DESKTOP_PATH="$DESKTOP_DIR/$APP_ID.desktop"

INSTALL_SERVICE=1
UNINSTALL=0

c() { printf '\033[%sm%s\033[0m' "$1" "$2"; }
say()  { printf '%s %s\n' "$(c '1;36' '==>')" "$*"; }
ok()   { printf '%s %s\n' "$(c '1;32' ' ok')" "$*"; }
warn() { printf '%s %s\n' "$(c '1;33' '  !')" "$*" >&2; }

usage() {
  cat <<USAGE
Install AI Translate (native KDE/Wayland translator).

Usage: ./install.sh [options]

  --no-service   Install the binary and app-menu entry only (no hotkey daemon).
  --uninstall    Remove the binary, service, and app-menu entry.
  -h, --help     Show this help.

Environment: BIN_DIR (default ~/.local/bin).
USAGE
}

for arg in "$@"; do
  case "$arg" in
    --no-service)        INSTALL_SERVICE=0 ;;
    --uninstall|--remove) UNINSTALL=1 ;;
    -h|--help)           usage; exit 0 ;;
    *) warn "unknown argument: $arg"; usage >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------- uninstall
if [[ "$UNINSTALL" == 1 ]]; then
  say "Uninstalling AI Translate"
  if systemctl --user list-unit-files "$APP_ID.service" >/dev/null 2>&1; then
    systemctl --user disable --now "$APP_ID.service" 2>/dev/null || true
  fi
  rm -f "$UNIT_PATH" "$DESKTOP_PATH" "$BIN_DIR/$BIN_NAME"
  systemctl --user daemon-reload 2>/dev/null || true
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
  ok "Removed binary, service, and app-menu entry."
  echo "   Your config in ~/.config/$BIN_NAME was left untouched."
  exit 0
fi

# ---------------------------------------------------------------- preflight
if [[ ! -x "$DIR/$BIN_NAME" ]]; then
  warn "Can't find the '$BIN_NAME' binary next to this script ($DIR)."
  warn "Run install.sh from inside the extracted tarball folder."
  exit 1
fi

say "Installing the binary"
mkdir -p "$BIN_DIR"
install -m755 "$DIR/$BIN_NAME" "$BIN_DIR/$BIN_NAME"
ok "$BIN_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on your PATH. Add this to ~/.bashrc or ~/.profile:"
     warn "    export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

# ---------------------------------------------------------------- .desktop
say "Adding the app-menu entry"
mkdir -p "$DESKTOP_DIR"
cat >"$DESKTOP_PATH" <<DESKTOP
[Desktop Entry]
Type=Application
Name=AI Translate
GenericName=Translator
Comment=Translate the selection, a screen region (OCR), or typed text
Exec=$BIN_DIR/$BIN_NAME
Icon=accessories-dictionary
Terminal=false
Categories=Utility;Office;
Keywords=translate;translation;ocr;dictionary;
StartupWMClass=ai-translate
Actions=Selection;OCR;

[Desktop Action Selection]
Name=Translate Selection
Exec=$BIN_DIR/$BIN_NAME selection

[Desktop Action OCR]
Name=Capture & Translate (OCR)
Exec=$BIN_DIR/$BIN_NAME ocr
DESKTOP
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
ok "$DESKTOP_PATH"

# ---------------------------------------------------------------- systemd unit
if [[ "$INSTALL_SERVICE" == 1 ]]; then
  say "Registering the global-hotkey daemon (systemd user service)"
  mkdir -p "$UNIT_DIR"
  cat >"$UNIT_PATH" <<UNIT
[Unit]
Description=AI Translate global-hotkey daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=$BIN_DIR/$BIN_NAME daemon
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
UNIT
  ok "$UNIT_PATH"

  if [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && systemctl --user show-environment >/dev/null 2>&1; then
    # Make sure the user manager sees the Wayland/X11/desktop env the daemon needs.
    systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XAUTHORITY 2>/dev/null || true
    systemctl --user daemon-reload
    systemctl --user enable --now "$APP_ID.service"
    ok "Service enabled and started."
  else
    warn "No user systemd session detected (headless shell?). Start it from your KDE session with:"
    warn "    systemctl --user daemon-reload && systemctl --user enable --now $APP_ID.service"
  fi
fi

# ---------------------------------------------------------------- runtime deps
echo
say "Optional runtime tools (install once, with apt)"
cat <<DEPS
  Selection read / clipboard : wl-clipboard   (wl-paste, wl-copy)   + xclip (X11)
  Region capture for OCR     : kde-spectacle  (Spectacle)
  OCR engine                 : tesseract-ocr  (+ language packs)
  Desktop notifications      : libnotify-bin  (notify-send)
  Window-at-cursor placement : qdbus-qt6      (provides /usr/bin/qdbus6; ships with KDE Plasma)

  sudo apt install wl-clipboard xclip kde-spectacle tesseract-ocr libnotify-bin qdbus-qt6
  # extra OCR languages, e.g.:  sudo apt install tesseract-ocr-chi-sim tesseract-ocr-jpn
DEPS

echo
ok "Done. AI Translate is installed."
cat <<NEXT

  Triggers are auto-assigned to the first free key (Meta+E / Meta+R / Meta+T
  preferred); your exact keys may differ. See / rebind them in
  System Settings > Shortcuts (component "AI Translate"):
    Meta+E  (else Ctrl+Alt+E / Meta+S)        translate the current selection
    Meta+R  (else Ctrl+Alt+R / Meta+Shift+O)  capture a screen region, OCR, translate
    Meta+T  (else Ctrl+Alt+G / Meta+Shift+T)  open the type/paste popup

  Config file:   $("$BIN_DIR/$BIN_NAME" config-path 2>/dev/null || echo "~/.config/$BIN_NAME/config.toml")
  Settings:      open the popup and click the gear (set AI key, proxy, etc.)
NEXT
