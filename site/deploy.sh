#!/bin/sh
# Deploy the SMARTAGENT landing page (site/) to the EC2 VM.
#
#   site/deploy.sh            bump cache version, rsync, verify live
#   site/deploy.sh --dry-run  show what would sync, change nothing
#
# Static site — no build step. "Build" here means: syntax-check main.js,
# bump the ?v= cache-buster in index.html so clients drop the old bundle,
# then rsync to the nginx root and verify the live version marker.
set -eu

HOST="akurai-mail"                      # ~/.ssh/config → ubuntu@mail.olibuijr.com
DOCROOT="/var/www/smartagent"
URL="https://smartagent.olibuijr.com"
SITE="$(cd "$(dirname "$0")" && pwd)"

say() { printf '%s\n' "$*"; }
die() { printf '✗ %s\n' "$*" >&2; exit 1; }

DRY=""
[ "${1:-}" = "--dry-run" ] && DRY="--dry-run"

# syntax gate: a broken main.js must never reach the VM
node -e "new Function(require('fs').readFileSync('$SITE/main.js','utf8').replace(/^import[^;]+;/gm,'').replace(/export /g,'').replace(/\bawait\b/g,''))" \
    || die "main.js does not parse — not deploying"
say "✔ main.js parses"

# bump ?v=N on the module script (cache-buster)
CUR="$(sed -n 's/.*main\.js?v=\([0-9]*\).*/\1/p' "$SITE/index.html")"
[ -n "$CUR" ] || die "no main.js?v=N marker in index.html"
NEXT=$((CUR + 1))
if [ -z "$DRY" ]; then
    sed -i "s/main\.js?v=$CUR/main.js?v=$NEXT/" "$SITE/index.html"
    say "✔ cache version: v=$CUR → v=$NEXT"
else
    say "dry-run: would bump v=$CUR → v=$((CUR + 1))"
fi

rsync -az --delete $DRY \
    --exclude 'deploy.sh' --exclude 'interceptor-screenshot-*' \
    "$SITE/" "$HOST:$DOCROOT/"
say "✔ rsync → $HOST:$DOCROOT"

[ -n "$DRY" ] && { say "dry-run complete"; exit 0; }

# verify: live HTML must carry the new version marker
LIVE="$(curl -fsS "$URL/" | sed -n 's/.*main\.js?v=\([0-9]*\).*/\1/p')"
[ "$LIVE" = "$NEXT" ] || die "live version is v=$LIVE, expected v=$NEXT"
CODE="$(curl -fsS -o /dev/null -w '%{http_code}' "$URL/main.js?v=$NEXT")"
[ "$CODE" = "200" ] || die "main.js fetch returned $CODE"
say "✔ live at $URL (v=$NEXT)"
