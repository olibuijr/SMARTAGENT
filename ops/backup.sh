#!/bin/sh
# Nightly backup of SMARTAGENT's durable state (agent memory, secrets, schedule,
# semdb tables). One disk failure or a crash-corrupted semdb file otherwise
# loses the whole brain. Keeps the last 7 dated tarballs off the repo tree.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${SMARTAGENT_BACKUP_DIR:-$HOME/.smartagent-backups}"
KEEP=7
mkdir -p "$DEST"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
TARBALL="$DEST/smartagent-data-$STAMP.tar.gz"

# data/ holds the semdb tables + secrets; skip if absent.
[ -d "$ROOT/data" ] || { echo "no data/ dir — nothing to back up"; exit 0; }

tar -czf "$TARBALL" -C "$ROOT" data
echo "backup: wrote $TARBALL ($(du -h "$TARBALL" | cut -f1))"

# Verify the archive is readable before pruning old ones.
tar -tzf "$TARBALL" >/dev/null || { echo "backup: archive verify FAILED" >&2; exit 1; }

# Retain only the newest $KEEP.
ls -1t "$DEST"/smartagent-data-*.tar.gz 2>/dev/null | tail -n +$((KEEP + 1)) | while read -r old; do
    rm -f "$old"
    echo "backup: pruned $old"
done
