#!/bin/sh
# Start/verify SMARTAGENT's external runtime deps and probe every endpoint in
# config/smartagent.conf. Run after a reboot, or before a work session, so the
# agent never discovers a dead dependency mid-task.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONF="$ROOT/config/smartagent.conf"

val() { sed -n "s/^$1[[:space:]]*=[[:space:]]*//p" "$CONF" | head -1; }
probe() { # name url
    if curl -s -m 4 -o /dev/null "$2"; then echo "  ok    $1 ($2)"; else echo "  DOWN  $1 ($2)"; fi
}

echo "SMARTAGENT preflight"

# SearXNG (docker, restart=unless-stopped)
if ! docker ps --format '{{.Names}}' 2>/dev/null | grep -q '^smartagent-searxng$'; then
    echo "  starting searxng container…"
    docker start smartagent-searxng >/dev/null 2>&1 || echo "  DOWN  searxng container (create it first)"
fi

# Headless Chromium CDP
if ! curl -s -m 3 -o /dev/null http://127.0.0.1:9222/json/version; then
    echo "  starting chromium (systemctl --user)…"
    systemctl --user start smartagent-chromium 2>/dev/null || \
        (chromium --headless=new --remote-debugging-port=9222 \
            --user-data-dir="$ROOT/.pi/chrome-profile" --no-first-run --disable-gpu >/dev/null 2>&1 &)
    sleep 3
fi

echo "endpoint probes:"
EMB="$(val embeddings_endpoint)"; [ -n "$EMB" ] && probe embeddings "http://$EMB/health"
SEARX="$(val searx_instance)"; [ -n "$SEARX" ] && probe searxng "$SEARX/healthz"
probe chromium-cdp http://127.0.0.1:9222/json/version
