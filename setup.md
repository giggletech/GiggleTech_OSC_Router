## Setup (quick + simple)


This is the shortest path to “it works in VRChat”. For full details, see `README.md`.

---

## 1) Put the device on Wi-Fi

- Power it on.
- Enter configuration mode (unplug/replug right after power-up; LED goes solid).
- Join Wi-Fi `Giggletech_haptics` (password `giggletech`).
- Open `http://192.168.4.1` and connect the device to your home Wi-Fi.
- Power-cycle the device.

You will need the device's **IP address** (from your router DHCP list, `http://giggletech.local`, or later via the app's Ping button).

---

## 2) Install OSCQuery helper (recommended)

- Run `Giggletech_OSCQuery_Installer.exe`.

This helps the router auto-detect VRChat's OSC listen port when `port_rx: OSCQuery` is used in `config.yml`.

---

## 3) Configure `config.yml`

Place `config.yml` next to `GiggleTech.exe`.

Minimum per device:

- `ip`: your device IP
- `proximity_parameter`: your avatar parameter name, like `proximity_01`

Optional (recommended):

- `online_parameter`: a Bool avatar parameter name like `HeadpatsOnline` (router sends true/false based on ping)

---

## 4) Run the router

- Launch `GiggleTech.exe`.
- Leave it running while in VRChat (closing the window keeps it in the **system tray**).

Helpful UI features:

- **VR MODE**: larger window for use inside a headset
- **Ping**: checks if each device is online/offline on your LAN

---

## 5) Verify in VRChat

- Make sure VRChat OSC is enabled.
- Confirm your avatar uses the same parameter names from `config.yml`.
- When you trigger proximity/pats, the device should respond.

If using `online_parameter`, power the device off/on and confirm your Bool parameter changes.

---

## Quick troubleshooting

- **Device shows Offline**: wrong IP, not on Wi-Fi, or ping blocked by firewall
- **No haptics**: wrong `proximity_parameter`, router not running, or `port_rx` mismatch (try `OSCQuery`)
- **Nothing updates in VRChat**: parameter names don't match, or VRChat OSC is off

Logs: `giggletech_log.txt` beside the executable.
