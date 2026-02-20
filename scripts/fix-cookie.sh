#!/bin/bash
# fix-cookie.sh — Quick cookie replacement for chaturbate-recorder
# Usage: ./fix-cookie.sh "cf_clearance=abc123..."
#
# Updates the cookie in podman-compose.yml and restarts the recorder container.

set -euo pipefail

COMPOSE_DIR="${COMPOSE_DIR:-/home/bert}"
COMPOSE_FILE="${COMPOSE_DIR}/podman-compose.yml"
SERVICE_NAME="${SERVICE_NAME:-chaturbate-recorder}"

if [ $# -lt 1 ]; then
    echo "Usage: $0 <cookie_value>"
    echo ""
    echo "  cookie_value: full cookie string, e.g.:"
    echo "    cf_clearance=abc123; sessionid=xyz789; csrftoken=tok456"
    echo ""
    echo "  Or just the cf_clearance value:"
    echo "    cf_clearance=abc123"
    echo ""
    echo "Environment:"
    echo "  COMPOSE_DIR   — directory containing podman-compose.yml (default: /home/bert)"
    echo "  SERVICE_NAME  — container service name (default: chaturbate-recorder)"
    exit 1
fi

COOKIE="$1"

if [ ! -f "$COMPOSE_FILE" ]; then
    echo "❌ Compose file not found: $COMPOSE_FILE"
    exit 1
fi

echo "🍪 Updating cookie in $COMPOSE_FILE..."

# If config.toml exists alongside compose, update it directly
CONFIG_FILE="${COMPOSE_DIR}/config.toml"
if [ -f "$CONFIG_FILE" ]; then
    # Update cookies line in config.toml
    if grep -q '^cookies\s*=' "$CONFIG_FILE"; then
        sed -i "s|^cookies\s*=.*|cookies = \"${COOKIE}\"|" "$CONFIG_FILE"
        echo "✅ Updated config.toml"
    else
        echo "⚠️  No 'cookies' line in config.toml — add it under [network]:"
        echo "    cookies = \"${COOKIE}\""
    fi
fi

echo "🔄 Restarting ${SERVICE_NAME}..."
cd "$COMPOSE_DIR"
podman-compose restart "$SERVICE_NAME" 2>/dev/null || podman restart "$SERVICE_NAME" 2>/dev/null || {
    echo "⚠️  Restart failed. Try manually:"
    echo "    cd $COMPOSE_DIR && podman-compose down && podman-compose up -d"
    exit 1
}

echo "✅ Done! Recorder restarted with new cookie."
echo "   Watch logs: podman logs -f ${SERVICE_NAME}"
