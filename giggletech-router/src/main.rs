/*
    GiggleTech.io - OSC Router
    by Sideways
    Based on OSC Async Library: https://github.com/Frando/async-osc
*/

use std::io;

mod config;
mod config_editor;
mod data_processing;
mod giggletech_osc;
mod handle_proximity_parameter;
mod log_ui;
mod osc_timeout;
mod router;
mod stop_pats;
mod terminator;

#[cfg(windows)]
mod tray;

fn log_to_file(message: &str) {
    log_ui::app_log(message);
}

fn main() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = format!("Application panicked: {}", panic_info);
        log_to_file(&message);
    }));

    log_to_file("Starting GiggleTech OSC Router...");

    let no_tray = std::env::args().any(|a| a == "--no-tray");

    #[cfg(windows)]
    if !no_tray {
        run_with_tray();
        return;
    }

    run_console_mode();
}

#[cfg(windows)]
fn run_with_tray() {
    log_ui::set_console_mirror(false);

    unsafe {
        use winapi::um::wincon::GetConsoleWindow;
        use winapi::um::winuser::{ShowWindow, SW_HIDE};
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_HIDE);
        }
    }

    std::thread::spawn(|| {
        async_std::task::block_on(async {
            let restart_rx = router::init_restart_channel();
            if let Err(e) = router::run_giggletech_loop(restart_rx).await {
                let error_message = format!("Application encountered an error: {}", e);
                log_to_file(&error_message);
            }
        });
    });

    tray::run();
}

fn run_console_mode() {
    log_ui::set_console_mirror(true);

    async_std::task::block_on(async {
        let restart_rx = router::init_restart_channel();
        if let Err(e) = router::run_giggletech_loop(restart_rx).await {
            let error_message = format!("Application encountered an error: {}", e);
            log_to_file(&error_message);
            eprintln!("{}", error_message);
        }

        println!("Press Enter to exit...");
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
    });
}

pub(crate) async fn test_device_connectivity(devices: &[config::DeviceConfig]) {
    log_ui::log_line("\n=== Testing Device Connectivity ===");
    log_to_file("Starting device connectivity test...");

    for (i, device) in devices.iter().enumerate() {
        let device_ip = &device.device_uri;

        log_ui::log_line(&format!("  Testing Device {}: {}", i + 1, device_ip));

        let is_reachable = ping_device(device_ip).await;

        let status = if is_reachable { "ONLINE" } else { "OFFLINE" };
        let message = format!("Device {}: {} - {}", i + 1, device_ip, status);

        log_ui::log_line(&format!("  Result: {}", message));
        log_to_file(&message);
        log_ui::log_line("");
    }

    log_ui::log_line("=== Connectivity Test Complete ===\n");
    log_to_file("Device connectivity test completed.");
}

async fn ping_device(device_ip: &str) -> bool {
    match async_std::process::Command::new("ping")
        .args(&["-n", "1", "-w", "1000", device_ip])
        .output()
        .await
    {
        Ok(output) => {
            let success = output.status.success();
            if success {
                log_ui::log_line(&format!("    ✓ Ping successful for {}", device_ip));
            } else {
                log_ui::log_line(&format!("    ✗ Ping failed for {}", device_ip));
            }
            success
        }
        Err(e) => {
            log_ui::log_line(&format!("    ✗ Ping command failed: {}", e));
            false
        }
    }
}
