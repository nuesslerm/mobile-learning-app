#!/usr/bin/with-contenv bash
# The login browser is headless: no display server, window manager or streamed desktop, only Chromium with CDP on loopback.
# The listener's viewer (`vnc-auth-listener`) takes frames and sends input over that CDP port.
#
# Every caller must end up headless with CDP on, however it found the binary: codex execs /usr/lib/chromium/chromium by absolute
# path, the rest go through $PATH or xdg-open. The only interception point that catches all of them is the real binary's own
# path, so it is moved aside once and replaced by a wrapper. The shipped `wrapped-chromium` (and the `chromium` symlink to it)
# gets the same flags for the callers that stop there.
#
# --kiosk fills the 393x852 screen and --force-device-scale-factor=1 keeps the CSS viewport at it. Flags a caller passes that we own are dropped: Chromium takes the LAST occurrence of a repeated flag, so a later copy
# from the caller would silently win. The profile lives in /tmp because there is nothing to restore across container starts.
set -e
CDP_PORT="${CDP_PORT:-9222}"
PROFILE_DIR="${PROFILE_DIR:-/tmp/chromium-profile}"
REAL_BIN="${REAL_BIN:-/usr/lib/chromium/chromium}"
REAL_BIN_BACKUP="${REAL_BIN_BACKUP:-/usr/lib/chromium/chromium.real}"
WRAPPER="${WRAPPER:-/usr/local/bin/wrapped-chromium}"
WRAPPER_BIN="${WRAPPER_BIN:-/usr/bin/chromium}"
CHROMIUM_SYMLINK="${CHROMIUM_SYMLINK:-/usr/local/bin/chromium}"
OWN_FLAGS="--remote-debugging-port=$CDP_PORT --kiosk --disable-gpu --disable-dev-shm-usage --hide-scrollbars --window-size=393,852 --force-device-scale-factor=1 --no-first-run --no-default-browser-check --user-data-dir=$PROFILE_DIR"
DROP_PATTERN='--remote-debugging-port=*|--user-data-dir=*|--headless*|--window-size=*|--start-fullscreen|--kiosk|--app=*|--load-extension=*'

if [ -f "$REAL_BIN" ] && [ ! -f "$REAL_BIN_BACKUP" ]; then
  mv "$REAL_BIN" "$REAL_BIN_BACKUP"
  cat > "$REAL_BIN" <<REAL_BIN_EOF
#! /bin/bash
ARGS=()
for arg in "\$@"; do
  case "\$arg" in
    $DROP_PATTERN) ;;
    *) ARGS+=("\$arg") ;;
  esac
done
exec $REAL_BIN_BACKUP $OWN_FLAGS "\${ARGS[@]}"
REAL_BIN_EOF
  chmod +x "$REAL_BIN"
  if grep -q -- '--kiosk' "$REAL_BIN"; then
    echo "[vnc-auth-init] kiosk wrapper installed at $REAL_BIN"
  else
    echo "[vnc-auth-init] real-binary wrapper has no --kiosk: absolute-path callers like codex would open an unframed window" >&2
  fi
else
  echo "[vnc-auth-init] $REAL_BIN missing or already wrapped ($REAL_BIN_BACKUP exists) -- skipping real-binary patch" >&2
fi

if [ -f "$WRAPPER" ]; then
  # Keep the shipped wrapper's seccomp branch: some container privilege configurations need --no-sandbox, others do not.
  cat > "$WRAPPER" <<WRAPPER_EOF
#! /bin/bash
if ! pgrep chromium > /dev/null; then
  rm -f \$HOME/.config/chromium/Singleton* $PROFILE_DIR/Singleton*
fi
ARGS=()
for arg in "\$@"; do
  case "\$arg" in
    $DROP_PATTERN) ;;
    *) ARGS+=("\$arg") ;;
  esac
done
if grep -q 'Seccomp:.0' /proc/1/status; then
  exec $WRAPPER_BIN --password-store=basic $OWN_FLAGS "\${ARGS[@]}"
else
  exec $WRAPPER_BIN --password-store=basic --no-sandbox --test-type $OWN_FLAGS "\${ARGS[@]}"
fi
WRAPPER_EOF
  chmod +x "$WRAPPER"
  # Covers tools that exec a plain `chromium` off $PATH: /usr/local/bin precedes /usr/bin, so this wins without touching the binary.
  ln -sf "$WRAPPER" "$CHROMIUM_SYMLINK"
  echo "[vnc-auth-init] $CHROMIUM_SYMLINK -> $WRAPPER (kiosk, CDP on $CDP_PORT)"
else
  echo "[vnc-auth-init] $WRAPPER not found -- base image layout changed, headless patch skipped" >&2
fi
