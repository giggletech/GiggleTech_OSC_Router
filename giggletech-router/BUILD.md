# Building GiggleTech OSC Router

This crate (`giggletech-router`) builds **GiggleTech.exe** — the VRChat OSC router with a settings window and system tray.

---

## Requirements

| Requirement | Notes |
|-------------|-------|
| **Rust** | Stable toolchain (1.70+ recommended). Install from [rustup.rs](https://rustup.rs/). |
| **Windows** | Primary target. Tray icon, single-instance guard, and embedded icon use Windows APIs (`winapi`, `winres`, `tray-icon`, `tao`, `wry`). |
| **MSVC build tools** | Required on Windows so `build.rs` can compile the `.ico` resource via `winres`. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload, or full Visual Studio. |

On non-Windows platforms the project may compile in console-only mode, but tray UI and several integrations are gated behind `#[cfg(windows)]`.

---

## Repository layout

```text
GiggleTech_OSC_Router/
├── Cargo.toml              # Workspace root (members: giggletech-router)
├── config.yml              # Example / dev config (also copied beside the exe for releases)
└── giggletech-router/
    ├── Cargo.toml          # Package name: async-osc, binary: GiggleTech
    ├── build.rs            # Embeds src/assets/bolt.ico on Windows
    └── src/
        ├── main.rs         # Entry point
        ├── router.rs       # OSC routing loop
        ├── tray.rs         # System tray + settings UI (Windows)
        └── ...
```

---

## Build from source

Open a terminal at the **workspace root** (`GiggleTech_OSC_Router/`).

### Debug build (faster compile, console visible)

```powershell
cargo build
```

Output: `target/debug/GiggleTech.exe`

### Release build (optimized, no console in normal tray mode)

```powershell
cargo build --release
```

Output: `target/release/GiggleTech.exe`

### Build only this crate

From the workspace root:

```powershell
cargo build --release -p async-osc
```

Or from `giggletech-router/`:

```powershell
cd giggletech-router
cargo build --release
```

---

## Run after building

### Default (Windows tray app)

```powershell
.\target\release\GiggleTech.exe
```

- Opens the settings window and system tray icon.
- Starts the OSC router in a background thread.
- In **release** builds the console window is hidden unless you pass flags below.

### Useful CLI flags

| Flag | Effect |
|------|--------|
| `--no-tray` | Skip tray UI; run in console mode (useful for debugging on Windows). |
| `--console` | Keep the console visible while using the tray UI. |
| `--autostart` | Start minimized (intended for login / Task Scheduler). |

Example:

```powershell
.\target\release\GiggleTech.exe --console
```

---

## Configuration

The router loads **config.yml** from (in order):

1. Current working directory
2. `giggletech-router/config.yml` (when running from the repo)
3. The folder containing `GiggleTech.exe`

For local development, either:

- Run from the repo root where `config.yml` already exists, or
- Copy `config.yml` next to `GiggleTech.exe` in `target/release/`.

See the root [README.md](../README.md) for full `config.yml` reference.

Logs are written to **giggletech_log.txt** beside the executable.

---

## Packaging a release folder

Minimal layout to ship or run on another PC:

```text
GiggleTech.exe
config.yml
```

Optional: **Giggletech_OSCQuery_Installer.exe** if using `port_rx: OSCQuery` (see root README).

---

## Troubleshooting builds

| Problem | Fix |
|---------|-----|
| `link.exe` not found / linker errors | Install MSVC Build Tools (C++ workload). |
| `Failed to compile Windows resources` | Confirm `giggletech-router/src/assets/bolt.ico` exists and build tools are installed. |
| Wrong binary name | The Cargo package is `async-osc`; the binary is **`GiggleTech`** (see `[[bin]]` in `Cargo.toml`). |
| Config not found at runtime | Place `config.yml` in the cwd or next to the exe; see `ensure_config_working_directory()` in `main.rs`. |

---

## Clean rebuild

```powershell
cargo clean
cargo build --release
```
