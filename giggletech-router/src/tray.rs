use std::{thread, time::Duration};

use tray_icon::{
    menu::{accelerator::Accelerator, Menu, MenuEvent, MenuId, MenuItem},
    TrayIconBuilder,
};
#[cfg(windows)]
use winapi::um::wincon::GetConsoleWindow;
#[cfg(windows)]
use winapi::um::winuser::{ShowWindow, SW_HIDE, SW_SHOW};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

pub fn setup_and_run_tray() {
    // Hide the terminal after 1 second
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        hide_terminal();
    });

    let icon = load_icon();
    let event_loop = EventLoopBuilder::new().build().unwrap();

    let menu: Menu = Menu::new();
    let show_terminal_id: MenuId = MenuId::new("1");
    let hide_terminal_id = MenuId::new("2");
    let quit_id: MenuId = MenuId::new("3");

    let _ = menu.append_items(&[
        &MenuItem::with_id(
            show_terminal_id.clone(),
            "Show Terminal",
            true,
            None::<Accelerator>,
        ),
        &MenuItem::with_id(
            hide_terminal_id.clone(),
            "Hide Terminal",
            true,
            None::<Accelerator>,
        ),
        &MenuItem::with_id(quit_id.clone(), "Quit", true, None::<Accelerator>),
    ]);

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
            if event.id == show_terminal_id {
                show_terminal();
            } else if event.id == hide_terminal_id {
                hide_terminal();
            } else if event.id == quit_id {
                std::process::exit(0);
            }
        }
    });
}

#[cfg(windows)]
fn show_terminal() {
    unsafe {
        let window = GetConsoleWindow();
        if window != std::ptr::null_mut() {
            ShowWindow(window, SW_SHOW);
        }
    }
}

#[cfg(windows)]
fn hide_terminal() {
    unsafe {
        let window = GetConsoleWindow();
        if window != std::ptr::null_mut() {
            ShowWindow(window, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn show_terminal() {
    // Other platform code or no-op
}

#[cfg(not(windows))]
fn hide_terminal() {
    // Other platform code or no-op
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
