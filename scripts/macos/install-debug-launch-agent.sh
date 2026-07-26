#!/usr/bin/env bash
# Optional macOS helper: install a per-user launchd agent that starts
# ChatGPT/Codex with loopback remote debugging so LaunchServices is less
# likely to drop Chromium flags.
#
# Default: ChatGPT Tools already uses `open -n -a App.app --args …` from the
# Rust launch path — most users do NOT need this agent.
#
# Usage:
#   ./scripts/macos/install-debug-launch-agent.sh          # install ChatGPT
#   ./scripts/macos/install-debug-launch-agent.sh Codex    # install Codex.app
#   ./scripts/macos/install-debug-launch-agent.sh --unload # remove agent
#
# Env:
#   CODEX_SKIN_PORT   debug port (default 9335)

set -euo pipefail

PORT="${CODEX_SKIN_PORT:-9335}"
LABEL="com.chatgpt-tools.remote-debug"
PLIST_DIR="${HOME}/Library/LaunchAgents"
PLIST="${PLIST_DIR}/${LABEL}.plist"

APP_NAME="ChatGPT"
if [[ "${1:-}" == "--unload" ]]; then
  launchctl bootout "gui/$(id -u)/${LABEL}" 2>/dev/null || true
  rm -f "${PLIST}"
  echo "Unloaded ${LABEL} and removed ${PLIST}"
  exit 0
fi
if [[ "${1:-}" == "Codex" || "${1:-}" == "codex" ]]; then
  APP_NAME="Codex"
fi

APP_PATH="/Applications/${APP_NAME}.app"
if [[ ! -d "${APP_PATH}" ]]; then
  APP_PATH="${HOME}/Applications/${APP_NAME}.app"
fi
if [[ ! -d "${APP_PATH}" ]]; then
  echo "error: ${APP_NAME}.app not found in /Applications or ~/Applications" >&2
  exit 1
fi

EXE="${APP_PATH}/Contents/MacOS/${APP_NAME}"
if [[ ! -x "${EXE}" ]]; then
  # Codex bundle sometimes ships as ChatGPT binary name
  if [[ -x "${APP_PATH}/Contents/MacOS/ChatGPT" ]]; then
    EXE="${APP_PATH}/Contents/MacOS/ChatGPT"
  else
    echo "error: executable not found under ${APP_PATH}/Contents/MacOS" >&2
    exit 1
  fi
fi

mkdir -p "${PLIST_DIR}"
cat > "${PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${EXE}</string>
    <string>--remote-debugging-port=${PORT}</string>
    <string>--remote-debugging-address=127.0.0.1</string>
  </array>
  <key>RunAtLoad</key>
  <false/>
  <key>KeepAlive</key>
  <false/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>StandardOutPath</key>
  <string>${HOME}/Library/Logs/chatgpt-tools-remote-debug.log</string>
  <key>StandardErrorPath</key>
  <string>${HOME}/Library/Logs/chatgpt-tools-remote-debug.err.log</string>
</dict>
</plist>
EOF

launchctl bootout "gui/$(id -u)/${LABEL}" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "${PLIST}"
echo "Installed ${PLIST}"
echo "Start once:  launchctl kickstart -k gui/$(id -u)/${LABEL}"
echo "Unload:      $0 --unload"
echo "Port:        ${PORT} (override with CODEX_SKIN_PORT)"
echo
echo "Note: ChatGPT Tools GUI already launches with open -a --args; use this"
echo "agent only if flag drop is still observed on your Mac."
