use std::{env, process::Command};
use tray_icon::{
    menu::{accelerator::Accelerator, Menu, MenuEvent, MenuItem},
    TrayIconBuilder,
};
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
};
use crate::path;
use std::process::exit;

pub fn setup_and_run_tray() {
    let icon = load_icon();
    let event_loop = EventLoopBuilder::new().build().unwrap();

    let menu: Menu = Menu::new();
    let open_terminal_item = MenuItem::new("Open Terminal", true, None::<Accelerator>);
    let restart_item = MenuItem::new("Restart", true, None::<Accelerator>);
    let quit_item = MenuItem::new("Quit", true, None::<Accelerator>);

    let _ = menu.append_items(&[&open_terminal_item, &restart_item, &quit_item]);

    let _tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("GiggleTech_OSC_Router")
            .with_icon(icon)
            .build()
            .unwrap(),
    );

    let menu_channel = MenuEvent::receiver();
    let event_loop_proxy = event_loop.create_proxy();

    let _ = event_loop.run(move |event, event_loop| {
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Ok(event) = menu_channel.try_recv() {
            match event.id {
                id if id == restart_item.id() => restart_application(),
                id if id == quit_item.id() => {
                    std::process::exit(0);
                },
                id if id == open_terminal_item.id() => {
                    // Signal the event loop to open a terminal-like window
                    let _ = event_loop_proxy.send_event(());
                }
                _ => (),
            }
        }
        if let Event::UserEvent(()) = event {
            let _ = open_terminal_with_logs();
        }
    });
}

fn open_terminal_with_logs() -> Result<(), Box<dyn std::error::Error>> {
    let log_file_path = path::join_exe_dir_with_file("/logs/output.log")?;
    let log_file_str = log_file_path.to_str().ok_or("Path contains invalid Unicode characters")?;
    
    Command::new("powershell")
    .args(&["-NoExit", "-Command", &format!("Get-Content '{}' -Wait", log_file_str)])
    .spawn() // Use spawn to launch the process without waiting for it to finish
    .expect("Failed to execute command");

    Ok(())
}

fn load_icon() -> tray_icon::Icon {
    const ICON_BYTES: &'static [u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../icon.ico"));
    load_icon_from_bytes(ICON_BYTES)
}

fn load_icon_from_bytes(bytes: &[u8]) -> tray_icon::Icon {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(bytes)
            .expect("Failed to load icon from bytes")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .expect("Failed to create icon from RGBA data")
}

fn restart_application() {
    let exe_path = path::current_exe_dir().unwrap();
    let exe_str = path::path_to_str(&exe_path).unwrap();

    #[cfg(windows)]
    let _ = Command::new(exe_str)
        .spawn()
        .expect("Application restart failed.");

    #[cfg(not(windows))]
    let _ = Command::new(exe_str)
        .spawn()
        .expect("Application restart failed.");

    exit(0);
}