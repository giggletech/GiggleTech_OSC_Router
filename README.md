# GiggleTech OSC Router

**Version 1.4** — Routes VRChat proximity OSC to GiggleTech haptic hardware, with optional device online status back to your avatar.

---

## What's included

| File | Purpose |
|------|---------|
| **config.yml** | Device IPs, VRChat parameters, speeds, and global setup |
| **GiggleTech.exe** | OSC router and settings window (system tray) |
| **Giggletech_OSCQuery_Installer.exe** | OSCQuery helper — discovers VRChat’s OSC listen port when using `OSCQuery` mode |

Logs are written to **giggletech_log.txt** beside the executable.

---

## Overview

1. Put your GiggleTech device on Wi‑Fi and note its IP.
2. Install the OSCQuery helper (if you use `OSCQuery` for `port_rx`).
3. Edit **config.yml** (IP, proximity parameters, optional online parameters).
4. Run **GiggleTech.exe** and keep it open while in VRChat.

The router listens for VRChat OSC (proximity, max speed, etc.), drives motors over UDP, and can report whether each device is reachable back to VRChat avatar parameters.

---

## Step 1: Configure your GiggleTech device

### Status LED

- **Power on**: 3 quick blinks.
- **Normal**: 3 blinks, 3 buzzes, then LED off (lights on pat commands).
- **Configuration mode**: Solid LED.

### Enter configuration mode

1. Power the device (3 blinks).
2. Unplug and plug back in — LED stays solid (configuration mode).

### Wi‑Fi setup

1. Join **Giggletech_haptics** (password: **giggletech**).
2. Open `http://192.168.4.1` if your phone does not open the portal automatically.
3. Choose your network and enter the Wi‑Fi password.

### IP setup

- **DHCP (recommended)**: Leave “Use static IP” unchecked, then Save.
- **Static IP**: Set IP, gateway, and subnet manually.

After saving, power-cycle the device. Three blinks and three buzzes mean it joined the network.

### Find the device IP

- Browse to `http://giggletech.local` (requires [Bonjour](https://developer.apple.com/bonjour/) on Windows if `.local` does not resolve), or  
- Check your router’s DHCP client list, or  
- Use the **Ping** button in the GiggleTech settings window once the router is running.

---

## Step 2: Software setup

1. Download the latest release from the GiggleTech GitHub.
2. Run **Giggletech_OSCQuery_Installer.exe** if you plan to use `port_rx: OSCQuery` in config (recommended for VRChat).
3. Place **config.yml** next to **GiggleTech.exe** (or in the folder you run from).
4. Launch **GiggleTech.exe**. A settings window and tray icon appear; the router starts automatically.

### VR mode

In the settings window header, click **VR MODE** to switch to a larger layout (wider panels, bigger controls) for use inside a headset. The choice is saved in the window and restored on next launch. Click again to return to normal size.

### Device online / offline in the UI

Each device card shows a status pill (**Online**, **Offline**, or **Checking**) updated about every 5 seconds via network ping. Use **Ping** on a device to refresh immediately. This is independent of VRChat; it only reflects whether the hardware answers on the LAN.

---

## Step 3: Configure `config.yml`

The router reads **config.yml** from (in order): the current working directory, `giggletech-router/config.yml`, or the folder containing **GiggleTech.exe**.

### Example

```yaml
devices:
  - name: Headpats
    ip: 192.168.1.69
    proximity_parameter: proximity_01
    online_parameter: HeadpatsOnline
    max_speed: 18
    use_velocity_control: true
    velocity_on_prox_drop: true
    outer_proximity: 0.13
    velocity_scalar: 22
    velocity_smoothing_ms: 145
  - ip: 192.168.1.70
    proximity_parameter: proximity_02
    online_parameter: Dev2Online
    max_speed: 22
    use_velocity_control: false

setup:
  port_rx: OSCQuery
  default_min_speed: 5
  default_max_speed: 25
  default_start_tx: 20
  default_max_speed_parameter: max_speed
  timeout: 5
  default_use_velocity_control: true
  default_velocity_on_prox_drop: false
  default_outer_proximity: 0.0
  default_inner_proximity: 1.0
  default_velocity_scalar: 20
  default_velocity_smoothing_ms: 80
  online_status_broadcast_seconds: 0
```

### `setup` (global)

| Key | Description |
|-----|-------------|
| `port_rx` | `OSCQuery` (use VRChat’s discovered port) or a fixed UDP port string, e.g. `"9001"` |
| `default_min_speed` / `default_max_speed` | Default motor power range as **percent** (5–100) for devices that omit `max_speed` |
| `default_start_tx` | Starting motor level when a pat begins (device TX scale) |
| `default_max_speed_parameter` | VRChat parameter name (without prefix) for global max speed, default `max_speed` |
| `timeout` | Seconds without proximity OSC before the motor is stopped for that device |
| `default_use_velocity_control` | Default: velocity-based haptics vs raw proximity level |
| `default_velocity_on_prox_drop` | Default: also fire on pull-away when velocity mode is on |
| `default_outer_proximity` / `default_inner_proximity` | Velocity band edges (0.0–1.0) |
| `default_velocity_scalar` | Velocity sensitivity (typical 1–100) |
| `default_velocity_smoothing_ms` | EMA smoothing for velocity (milliseconds) |
| `online_status_broadcast_seconds` | **0** = send online OSC only when ping state **changes**. **> 0** = also resend every N seconds (useful for debugging avatar parameters) |

### `devices` (per hardware unit)

| Key | Required | Description |
|-----|----------|-------------|
| `ip` | Yes | Device IP on your LAN |
| `proximity_parameter` | Yes | VRChat parameter name only, e.g. `proximity_01` → `/avatar/parameters/proximity_01` |
| `online_parameter` | No | VRChat **Bool** parameter for reachability, e.g. `HeadpatsOnline` → `/avatar/parameters/HeadpatsOnline` |
| `name` | No | Label in the settings UI (first device defaults to “Headpats” if empty) |
| `max_speed` | No | Power cap %; uses `default_max_speed` if omitted |
| `speed_scale` | No | Extra scale 0–100% |
| `max_speed_parameter` | No | Per-device max-speed VRChat parameter |
| `use_velocity_control` | No | Overrides `default_use_velocity_control` |
| `velocity_on_prox_drop` | No | Overrides default pull-away behavior |
| `outer_proximity` / `inner_proximity` | No | Velocity band; inner must be greater than outer |
| `velocity_scalar` | No | Overrides default velocity sensitivity |
| `velocity_smoothing_ms` | No | Overrides default smoothing |

Use `null` in YAML for optional fields you want to leave unset.

> **Note:** The settings UI edits IP, proximity, power, and velocity fields. **`online_parameter` and `online_status_broadcast_seconds` are edited in `config.yml` directly** so they are not cleared when you save from the window.

---

## Device online → VRChat OSC

When `online_parameter` is set for a device, the router periodically **pings** that device’s IP. It sends a **boolean** OSC message to VRChat at **`127.0.0.1:9000`** (VRChat’s default OSC port):

- **`true`** — device responded to ping (online on LAN)
- **`false`** — device did not respond (offline or wrong IP)

When a device **comes online**, the router briefly sends `false` then `true` so VRChat registers a clean transition on Bool parameters.

### Avatar setup in VRChat

1. In your avatar’s OSC or parameters setup, add a **Bool** parameter whose name matches `online_parameter` (e.g. `HeadpatsOnline`).
2. Use that parameter in Animator or menu logic (e.g. hide props, show “device disconnected”, gate haptics).
3. Ensure **OSC** is enabled in VRChat and the router is running on the same PC as VRChat.

State changes are logged in the router window, e.g. `/avatar/parameters/HeadpatsOnline true`.

---

## Step 4: Test before VRChat

1. Open **GiggleTech.exe** and confirm devices show **Online** after ping (fix IP or Wi‑Fi if not).
2. Optionally run **giggletech_vrc_simulator.exe** to emulate VRChat OSC without launching the game.
3. In VRChat, verify proximity parameters move your hardware and, if configured, online parameters update when you power a device off or on.

> **GiggleTech must stay running** while you play so proximity OSC is forwarded and online status is updated.

For help, use the GiggleTech Discord or support email from the release page.

---

## Version 1.4 highlights

- Graceful handling when devices are offline (no crash on unreachable networks)
- Background **ping** monitoring and optional **VRChat online OSC** per device
- Connection manager for UDP to hardware with improved error logging
- More reliable **OSCQuery** startup with fallback to port `9001`
- **VR mode** UI scaling in the settings window

See **change_log.txt** for earlier releases (velocity control, YAML config, motor start speed, etc.).

---

# GiggleTech device OSC API (direct control)

For custom tools or non-VRChat use, you can send OSC directly to the hardware.

## Requirements

- GiggleTech hardware (GigglePuck, GiggleSpark, etc.)
- An OSC-capable language or tool
- Same LAN as the device

## Paths

| OSC address | Description | Devices |
|-------------|-------------|---------|
| `/motor` | Motor intensity `0–255` (integer) | GigglePuck, GiggleSpark |

Default UDP port: **8888**.

## Motor scaling

Values are scaled by **0.66** for motor safety (5 V vs 3.3 V design):

```text
Effective output ≈ 0.66 × input (0–255)
```

Example: input `255` → effective `168`.

If OSC stops, the motor holds the last value until a new message arrives; resend periodically if you need a heartbeat.

## Troubleshooting (direct OSC)

| Issue | What to check |
|-------|----------------|
| No motor response | IP, port `8888`, path `/motor` |
| Stale behavior | Resend values; verify Wi‑Fi and device online |

---

## Troubleshooting (router / VRChat)

| Issue | What to check |
|-------|----------------|
| No haptics in VRChat | Router running; `port_rx` matches VRChat (try `OSCQuery`); proximity parameter names match avatar |
| Device always offline in UI | Wrong IP; device asleep; PC firewall blocking ping |
| Online parameter never changes | `online_parameter` set in YAML; Bool parameter exists on avatar; VRChat OSC on; same PC as router |
| Config errors on start | YAML syntax; required `ip` and `proximity_parameter` per device; see **giggletech_log.txt** |
