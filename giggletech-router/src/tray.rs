use std::env;
use tray_icon::{
    menu::{accelerator::Accelerator, Menu, MenuEvent, MenuItem},
    TrayIconBuilder,
};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

pub fn setup_and_run_tray() {
    let icon = load_icon();
    let event_loop = EventLoopBuilder::new().build().unwrap();

    let menu: Menu = Menu::new();
    let restart_item = MenuItem::new("Restart", true, None::<Accelerator>);
    let quit_item = MenuItem::new("Quit", true, None::<Accelerator>);

    let _ = menu.append_items(&[&restart_item, &quit_item]);

    let _tray_icon = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("GiggleTech_OSC_Router")
            .with_icon(icon)
            .build()
            .unwrap(),
    );

    let menu_channel = MenuEvent::receiver();
    let _ = event_loop.run(move |event, event_loop| {
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == restart_item.id() {
                restart_application();
            } else if event.id == quit_item.id() {
                std::process::exit(0);
            }
        }
    });
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

fn get_executable_path() -> std::io::Result<std::path::PathBuf> {
    env::current_exe()
}

fn restart_application() {
    use std::process::{exit, Command};

    // Get the current executable path
    let exe_path = match get_executable_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to get the current executable path: {}", e);
            return;
        }
    };

    // Convert the path to a string, if possible
    let exe_str = match exe_path.to_str() {
        Some(s) => s,
        None => {
            eprintln!("Failed to convert executable path to string.");
            return;
        }
    };

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
