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
