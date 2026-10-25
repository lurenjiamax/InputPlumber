# Experimental Thor InputPlumber release

For AYN Thor / SM8550 on Thorch (Arch Linux ARM). This downstream release adds
optional native SSC motion, shared translated rumble slots and per-device
rumble gain. It is not an official ShadowBlip release or a suspend fix.

## Install on Thorch

Download the aarch64 `thorch-inputplumber` package, `SHA256SUMS`, `PROVENANCE`
and `INSTALL.md` from the same release. Verify the downloaded files against
`SHA256SUMS` (or download all listed assets and run `sha256sum -c SHA256SUMS`).
Packages are unsigned; checksums do not authenticate the publisher.

Retain the currently installed package and configuration for rollback. Close
games and keep SSH or alternate input available; restarting interrupts input.
Set `PACKAGE` to the downloaded `.pkg.tar.zst` path, then run:

```sh
sudo pacman -U "$PACKAGE"
sudo systemctl restart inputplumber.service
systemctl is-active inputplumber.service
journalctl -b -u inputplumber.service --no-pager -n 40
```

Do not install this alongside a second InputPlumber daemon. Inspect existing
`/etc/inputplumber` and systemd overrides: older device definitions or custom
executable paths can take precedence over the packaged defaults.

## Vibration and motion

Vibration also requires a corrected Thor kernel/DTS (1300 mV, LRA mode and
atomic-context-safe haptics bridge). InputPlumber cannot supply those fixes.
Use a short low-gain preview first and confirm it stops.

Motion additionally requires a compatible FastRPC kernel and the separately
provisioned `thorch-motion` runtime, including this device's original private
firmware/calibration. No private materials are included. The corrected private
libssc path is `/usr/lib/thorch-motion/ssc/libssc.so.2`; no system library is
replaced. Select `ds5` using the existing target-switch interface for SDL motion;
the default Xbox-style target does not expose gyro. Closing games before target
switching avoids stale controller handles. Hardware frame/calibration and full
suspend/resume acceptance remain unresolved.

The upstream-layout tarball is for manual integration by maintainers and does
not replace the Arch package manager. Do not extract it over the live rootfs.
The embedded application version remains the upstream base version; the Thor
release tag and Arch package version identify the downstream release.

Rollback: reinstall the retained compatible package with pacman, restore only
the intended local overrides and restart `inputplumber.service`.
