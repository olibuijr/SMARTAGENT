#!/usr/bin/env bash
set -euo pipefail
command -v akurai-ec2 >/dev/null
akurai-ec2 --help >/dev/null
akurai-ec2 status
