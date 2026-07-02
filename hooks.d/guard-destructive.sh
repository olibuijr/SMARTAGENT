#!/bin/sh
# Block obviously destructive commands before the sandbox even sees them.
payload="$(cat)"
for bad in "rm -rf /" "mkfs" "dd if=" ":(){ :|:& };:"; do
    case "$payload" in
        *"$bad"*) echo "destructive pattern '$bad' refused by hook" >&2; exit 2 ;;
    esac
done
exit 0
