---
status: accepted
---

# Adaptive System Rate on Windows through the undocumented IPolicyConfig

The Desktop Client runs on Windows next to Linux, and its Output Modes keep their meaning there. On Linux, Adaptive System Rate forces the PipeWire graph to each Playback Source's sample rate. Windows mixes every application in shared mode at the default render endpoint's "Default Format", and has no documented API for an application to change it; the only documented path to the source rate is exclusive mode, which is already Exclusive.

So on Windows, Adaptive sets that endpoint's shared-mode format to the Playback Source's sample rate through `IPolicyConfig::SetDeviceFormat`, the interface the Sound control panel uses, which Microsoft does not document (`desktop/core/src/policy_config.rs`). Like the PipeWire graph rate, the change is system-wide: every application on the endpoint plays at that rate while Adaptive is on.

## Containing an undocumented interface

- Only the first four vtable entries (`GetMixFormat`, `GetDeviceFormat`, `ResetDeviceFormat`, `SetDeviceFormat`) are declared. COM calls by position, and the methods after these have changed shape between Windows releases.
- If the interface cannot be created, enabling Adaptive fails up front with its own message; Normal and Exclusive never touch it.
- The endpoint's original device and mix formats are written to `adaptive-system-rate.windows-format.json` before the first change. They are restored when Adaptive turns off or the client quits, and at the next start after a crash (the existing cleanup marker triggers the reset). An endpoint that is gone keeps its saved formats until it returns.
- A rate the hardware does not accept (`IAudioClient::IsFormatSupported`, exclusive mode) keeps the original format, and Windows resamples, rather than failing the Track.

## Considered Options

- Report Adaptive unavailable on Windows and point to Exclusive: rejected. Exclusive takes the device from every other application, which is what Adaptive exists to avoid.
- Have mpv request exclusive mode only for rate changes: rejected for the same reason; it is still exclusive.
- Ask the listener to change the Default Format by hand: rejected. It defeats following each Track, and the listener has to remember to change it back.

## Consequences

- A Windows update may break Adaptive without warning. It then reports itself unavailable; nothing else depends on the interface.
- The format is a per-endpoint setting, not a per-account one, so other Windows accounts on the machine see it while Adaptive is on. If the client never starts again after a crash, the listener restores the format in Sound settings.
- Adaptive matches the rate, not the samples. Windows still mixes in 32-bit float and applies volume and endpoint effects. Bit-perfect output on Windows remains Exclusive's job.
- Microsoft Store certification may reject undocumented API use; direct distribution is unaffected.
