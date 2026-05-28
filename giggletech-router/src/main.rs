/*
    GiggleTech.io - OSC Router
    by Sideways
    Based on OSC Async Library: https://github.com/Frando/async-osc
*/

// Hide the Windows console window in release builds (tray app).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io;

mod config;
mod config_editor;
mod device_ping;
mod device_test;
mod data_processing;
mod giggletech_osc;
mod handle_proximity_parameter;
mod log_ui;
mod osc_timeout;
mod router;
mod stop_pats;
mod terminator;
mod vrc_osc;

#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod tray;

fn main() {
    // Login/auto-start often sets cwd to System32; use the exe folder when config lives there.
    ensure_config_working_directory();

    std::panic::set_hook(Box::new(|panic_info| {
        let message = format!("Application panicked: {}", panic_info);
        log_ui::status(&message);
        eprintln!("{}", message);
    }));

    log_ui::status("Starting GiggleTech OSC Router...");

    let no_tray = std::env::args().any(|a| a == "--no-tray");
    let show_console = no_tray || std::env::args().any(|a| a == "--console");
    #[cfg(windows)]
    if !no_tray {
        let Some(primary) = single_instance::PrimaryInstance::acquire() else {
            single_instance::request_show_output_from_running_instance();
            return;
        };
        let start_minimized = std::env::args().any(|a| a == "--autostart");
        run_with_tray(show_console, start_minimized, primary);
        return;
    }

    run_console_mode();
}

/// If `config.yml` is not in the process cwd but is beside the executable, chdir there.
fn ensure_config_working_directory() {
    let cwd_has_config = std::env::current_dir()
        .map(|cwd| cwd.join("config.yml").is_file())
        .unwrap_or(false);
    if cwd_has_config {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    if dir.join("config.yml").is_file() {
        let _ = std::env::set_current_dir(dir);
    }
}

#[cfg(windows)]
fn run_with_tray(
    show_console: bool,
    start_minimized: bool,
    primary: single_instance::PrimaryInstance,
) {
    log_ui::set_console_mirror(show_console);

    if !show_console {
        unsafe {
            use winapi::um::wincon::GetConsoleWindow;
            use winapi::um::winuser::{ShowWindow, SW_HIDE};
            let hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    std::thread::spawn(|| {
        async_std::task::block_on(async {
            let restart_rx = router::init_restart_channel();
            if let Err(e) = router::run_giggletech_loop(restart_rx).await {
                let error_message = format!("Application encountered an error: {}", e);
                log_ui::status(&error_message);
            }
        });
    });

    tray::run(start_minimized, primary);
}

fn run_console_mode() {
    log_ui::set_console_mirror(true);

    async_std::task::block_on(async {
        let restart_rx = router::init_restart_channel();
        if let Err(e) = router::run_giggletech_loop(restart_rx).await {
            let error_message = format!("Application encountered an error: {}", e);
            log_ui::status(&error_message);
            eprintln!("{}", error_message);
        }

        println!("Press Enter to exit...");
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
    });
}

pub(crate) async fn test_device_connectivity(devices: &[config::DeviceConfig]) {
    log_ui::status("Checking device connectivity...");

    for (i, device) in devices.iter().enumerate() {
        let device_ip = &device.device_uri;
        let is_reachable = device_ping::ping_host(device_ip).await;
        let label = if is_reachable { "online" } else { "offline" };
        log_ui::status(&format!("Device {} ({}): {}", i + 1, device_ip, label));
    }
}
