#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
. scripts/_bundle.sh
scratch="target/test-scratch/bundle-verify-$$"
rm -rf "$scratch"
mkdir -p "$scratch"
trap 'rm -rf "$scratch"' EXIT
printf 'same bytes\n' > "$scratch/good.tar.gz"
cp "$scratch/good.tar.gz" "$scratch/evil.tar.gz"
hash=$(sha256sum "$scratch/good.tar.gz" | awk '{print $1}')
printf '%s  evil.tar.gz\n' "$hash" > "$scratch/SHA256SUMS"
if bundle_verify "$scratch/good.tar.gz" "$scratch/SHA256SUMS" 2>"$scratch/err"; then
  echo 'expected basename mismatch to fail' >&2
  exit 1
fi
printf '%s  good.tar.gz\n' "$hash" > "$scratch/SHA256SUMS"
bundle_verify "$scratch/good.tar.gz" "$scratch/SHA256SUMS"
