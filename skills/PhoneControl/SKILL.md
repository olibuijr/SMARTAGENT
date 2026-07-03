---
name: PhoneControl
description: >
  Drive Óli's connected Android phone (CPH2645, API 36, arm64) over wireless adb
  to build/install/verify the SMARTAGENT OS app on real hardware. Use when the
  task says "on the phone", "install on device", "screenshot the phone", "tap
  the phone", "test the APK", "check it on Android", or "scrcpy". The phone is
  the verification surface for `os/` (Dioxus mobile) the way Interceptor is for
  web. Screenshot → Read the PNG → tap by coords, exactly like Interceptor.
---

The phone is a real device on wireless adb (TLS/mDNS), so its id is a long
`adb-…._adb-tls-connect._tcp` string — never hardcode it. The `padb` helper
(on PATH, `~/.local/bin/padb`) resolves it dynamically and wraps every op.

## Verify loop (like Interceptor for web)

```sh
padb shot .scratch/phone.png     # capture — then Read the PNG to SEE the screen
padb tap 640 470                 # tap at device-pixel coords (screen is 1264x2780)
padb swipe 640 1800 640 600 250  # scroll up
padb key BACK                    # navigation keys
```

Read the PNG after every screenshot — that is how you confirm what rendered,
identical to reading an Interceptor screenshot. Multiply displayed-image coords
back to device pixels before tapping.

## Build → install → launch the OS app

```sh
# Build the APK (sccache MUST be disabled or dx's rustc probe fails):
RUSTC_WRAPPER="" ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.0.12077973 \
  dx build --platform android            # from os/
padb install <path-to.apk>
padb launch com.olibuijr.smartagent      # package from os Dioxus config
padb log SMARTAGENT                       # watch app logs (android_logger tag)
```

## Live mirror for Óli

`scrcpy` (installed) mirrors the screen with full mouse/keyboard control — hand
it to Óli when he wants to drive it himself.

## Gotchas

- **Battery:** check the status bar in the first screenshot; ask Óli to plug in
  before long install/test cycles (was at 15% on 2026-07-03).
- **Reconnect:** if `padb dev` shows nothing, `adb connect 192.168.1.235:<port>`
  (the wireless-debugging port rotates — re-enable from Developer Options).
- **Scratch only:** pulled files go under `.scratch/`, never `/tmp`.
- Device/target facts and the Dioxus decision live in memories
  `smartagent-phone-control` and `smartagent-os-dioxus-frontend`.
