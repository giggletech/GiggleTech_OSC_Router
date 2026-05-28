# Release notes (since v1.4)

This document summarizes changes from **`v1.4`** (`97a1d6e`, 2025-06-26) up to the current `HEAD`.

## Highlights

- **New system tray app + settings window** (major UI overhaul, large UX upgrade).
- **Config editor UI** for editing most common `config.yml` fields.
- **VR mode** UI scaling for in-headset readability.
- **Device online monitoring** via background ping, with optional **VRChat Bool OSC** output per device.
- **Device test / VRChat simulator tooling** to validate without launching VRChat.
- **Velocity control + smoothing improvements** and broader routing refinements.
- **New assets & icon** for the Windows app.
- **Twitch bot folder** added (separate Node.js project).

## Notable new/changed components

- **Tray / UI**
  - Added `giggletech-router/src/tray.rs` (large tray + UI implementation).
  - Added `giggletech-router/src/log_ui.rs` for in-app logging display.
  - Added `giggletech-router/src/config_editor.rs` for editing config from the UI.
  - Added app assets: `giggletech-router/src/assets/bolt.ico`, `giggletech-router/src/assets/Giggletech_Black.png`.

- **Routing / reliability**
  - Added `giggletech-router/src/router.rs` (routing/connection management refactor).
  - Added `giggletech-router/src/osc_timeout.rs` changes (timeout behavior updates).
  - Added `giggletech-router/src/single_instance.rs` (prevents multiple instances).

- **Device status**
  - Added `giggletech-router/src/device_ping.rs` for periodic ping monitoring.
  - `config.yml` / config parsing updated to support online status features.

- **Testing utilities**
  - Added `giggletech-router/src/device_test.rs` (device testing / simulation support).

- **VRChat integration**
  - Added `giggletech-router/src/vrc_osc.rs` (VRChat OSC output helpers).

- **Docs / tooling**
  - `README.md` expanded significantly and now documents v2.0 behavior, VR mode, config keys, and online OSC.
  - Added `setup.md` for step-by-step setup guidance.
  - Added `TODO.md`.

- **Twitch bot (separate project)**
  - Added `twitch-bot/` with `bot.js`, `package.json`, `package-lock.json`, `README.md`, and `supabase.sql`.

## File-level change summary (since v1.4)

In total (from git diff stats): **34 files changed**, with **~5315 insertions** and **~610 deletions**.

Added files include (selection):

- `giggletech-router/src/tray.rs`
- `giggletech-router/src/config_editor.rs`
- `giggletech-router/src/router.rs`
- `giggletech-router/src/single_instance.rs`
- `giggletech-router/src/device_ping.rs`
- `giggletech-router/src/device_test.rs`
- `giggletech-router/src/vrc_osc.rs`
- `giggletech-router/src/log_ui.rs`
- `twitch-bot/*`

Modified files include (selection):

- `giggletech-router/src/main.rs`
- `giggletech-router/src/config.rs`
- `giggletech-router/src/data_processing.rs`
- `giggletech-router/src/giggletech_osc.rs`
- `giggletech-router/src/handle_proximity_parameter.rs`
- `config.yml`
- `README.md`

## GitHub merge notes

- Includes merged PR: **#18** “patch-1”.

