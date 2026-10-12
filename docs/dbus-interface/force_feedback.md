# Force feedback

Composite devices expose `org.shadowblip.Output.ForceFeedback` on the system
bus. `Enabled` controls forwarding, `Rumble(value)` requests a simple rumble,
and `Stop()` sends a zero-amplitude rumble request.

## RumbleGain

`RumbleGain` is a read/write D-Bus double, scoped to a composite device. Its
default is `1.0`, preserving existing behavior. Valid values are finite numbers
in the inclusive range `0.0` to `2.0`. Invalid values are rejected, not clamped.
Changing the property uses the same PolicyKit authorization as `Enabled`.

- `0.0`: silence rumble amplitudes without disabling other feedback.
- `0.5`: halve rumble amplitudes.
- `1.0`: unchanged.
- `1.5`: increase amplitudes by 50%, saturating at the protocol maximum.

Strong and weak motors are scaled independently. Zero stays zero, and effect
IDs, playback/stop events, timing and report flags are not changed. Scaling is
applied once before fan-out to physical source devices, for generic rumble,
Steam Deck rumble, DualSense rumble fields and uploaded evdev `FF_RUMBLE`
effects. Trackpad haptics, adaptive triggers and non-rumble effects are outside
the scope of this property.

Original uploaded effect data is retained so changing gain can update existing
effects without compounding previous scaling. If a source rejects an update,
the setter reports the failure; the new gain remains applied for future output
and updates already accepted by other sources are not rolled back. Immediate
changes to an effect already playing depend on the physical driver. Streaming
reports use the new gain on their next report; changing the property does not
replay the last report.

The value is runtime-only and resets when the composite device is recreated.
Clients may persist their preference and restore it. Game-side gain handling
is separate; this property does not implement Linux `FF_GAIN` forwarding.

For example, substitute the actual composite-device object path:

```sh
busctl --system get-property org.shadowblip.InputPlumber \
  /org/shadowblip/InputPlumber/CompositeDevice0 \
  org.shadowblip.Output.ForceFeedback RumbleGain
busctl --system set-property org.shadowblip.InputPlumber \
  /org/shadowblip/InputPlumber/CompositeDevice0 \
  org.shadowblip.Output.ForceFeedback RumbleGain d 0.5
```
