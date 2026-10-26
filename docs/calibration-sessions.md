# Physical evdev calibration sessions

The `org.shadowblip.Input.Source.EventDevice` interface exposes a source-local
calibration transport. This is independent of input profiles and target device
types. Only running evdev gamepad sources support it. A privileged hardware
backend calculates and applies device-specific parameters; InputPlumber never
executes calibration scripts or writes driver sysfs parameters.

## API

* `BeginCalibration()` requires UID 0 and binds the session to the caller's
  unique system-bus connection. A second session is rejected.
* `ReadCalibration()` returns `(ready, overflow, axes, events)`. Axis entries are
  `(code, value, minimum, maximum, fuzz, flat, resolution)`; event entries are
  `(type, code, value)`. Values come from evdev before translation, normalization
  or capability/profile processing. They still include transformations performed
  by the kernel driver: these are not unprocessed ADC measurements.
* `RefreshCalibration()` asks the source thread to re-query kernel axis metadata.
  `ready` remains false until that refresh completes.
* `EndCalibration()` releases the session. The source refreshes kernel axis
  ranges and reconstructs its configured translator before resuming routing.

Read, refresh and end require the owning connection. Do not call these methods
through separate short-lived `busctl` processes: their D-Bus connections differ.
The client should keep one connection open for the whole session, poll until
`ready` before starting measurements, and read regularly. The queue holds up to
4096 raw events; `overflow` means a measurement was lost and the current
calibration step must be repeated.

## Lifecycle

The physical device stays grabbed and hidden by InputPlumber. Its normal input
routing is suspended; previously emitted buttons/axes are neutralized and rumble
is stopped on entry. Other sources and target device objects remain running.
The configured profile and target selection are never changed.

Every 250 ms the service checks whether the owning unique bus name still exists.
Disconnection, including client crashes, releases the session. Source removal
destroys its calibration state; a reconnected source requires a new session.
A bus liveness-query failure also releases the source. The method reply from
`EndCalibration` acknowledges release; source-thread range refresh occurs on the
next poll. Driver failures during refresh use the usual source error handling.

This initial transport is scoped to evdev gamepad sources. Hidraw-only controllers
need an equivalent driver-specific raw transport before they can use this API.

## Local Thor packaging

`pkg/archlinux/thorch/PKGBUILD` packages the checkout's cross-built aarch64
executable and maintained rootfs. Set `THORCH_INPUTPLUMBER_PACKAGE` to the existing
Thorch package directory; its payload and restart drop-in remain the single
maintenance location for Thor's controller layout and service settings.
The package includes the executable checksum, source revision and local diff so
existing unrelated hardware fixes included in a local build remain traceable.

Build with an aarch64 GNU linker, a recent Rust toolchain and a target libudev
sysroot. Set `PKG_CONFIG_SYSROOT_DIR`, `PKG_CONFIG_LIBDIR` and the target C compiler
include path to that sysroot. Then package from the checkout root:

```sh
PKGDEST="$PWD/artifacts" BUILDDIR="$PWD/build" \
  makepkg --config "$PWD/pkg/archlinux/thorch/makepkg.conf" \
  --nodeps --force --dir pkg/archlinux/thorch
```

Install with `pacman -U`. Restart InputPlumber once to load the new API, with GPcal
closed. No calibration values or saved calibration scripts need to be replaced.
