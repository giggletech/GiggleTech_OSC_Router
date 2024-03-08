use crate::logger;
use crate::path;
use log::error;
use std::{
    collections::HashMap,
    env,
    process::exit,
    process::{Child, Command},
};
use tray_icon::{
    menu::{accelerator::Accelerator, Menu, MenuEvent, MenuItem},
    TrayIconBuilder,
};
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
};

pub(crate) struct TrayApplication {
    powershell_processes: HashMap<String, Child>,
}

impl TrayApplication {
    pub fn new() -> Self {
        Self {
            powershell_processes: HashMap::new(),
        }
    }

    pub fn setup_and_run_tray(&mut self) {
        let icon = self.load_icon();
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
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Ok(event) = menu_channel.try_recv() {
                match event.id {
                    id if id == restart_item.id() => self.restart_application(),
                    id if id == quit_item.id() => self.quit_application(),
                    id if id == open_terminal_item.id() => {
                        let _ = event_loop_proxy.send_event(());
                    }
                    _ => (),
                }
            }
            if let Event::UserEvent(()) = event {
                let _ = self.open_terminal_with_logs();
            }
        });
    }

    fn open_terminal_with_logs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let log_file_path = path::join_exe_dir_with_file(logger::LOG_FILE)?;
        let log_file_str = log_file_path
            .to_str()
            .ok_or("Path contains invalid Unicode characters")?;

        let child = Command::new("powershell")
            .args(&[
                "-NoExit",
                "-Command",
                &format!("Get-Content '{}' -Wait -Encoding UTF8", log_file_str),
            ])
            .spawn() // Use spawn to launch the process without waiting for it to finish
            .expect("Failed to execute command");

        // Save the process to a HashMap so we can kill during quits and restarts
        let key = format!(
            "powershell-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
        );
        self.powershell_processes.insert(key, child);

        Ok(())
    }

    fn close_terminals(&mut self) {
        for (_key, child) in self.powershell_processes.iter() {
            let pid = child.id(); // Get the PID of the child process
            let _ = Command::new("taskkill")
                .args(&["/PID", &pid.to_string(), "/F"])
                .output()
                .map_err(|e| error!("Failed to execute taskkill: {:?}", e))
                .and_then(|output| {
                    if !output.status.success() {
                        Err(error!("taskkill failed: {:?}", output))
                    } else {
                        Ok(())
                    }
                });
        }
        self.powershell_processes.clear();
    }

    fn load_icon(&mut self) -> tray_icon::Icon {
        const ICON_BYTES: &'static [u8] =
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../icon.ico"));
        self.load_icon_from_bytes(ICON_BYTES)
    }

    fn load_icon_from_bytes(&mut self, bytes: &[u8]) -> tray_icon::Icon {
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

    fn quit_application(&mut self) {
        self.close_terminals();
        exit(0);
    }

    fn restart_application(&mut self) {
        self.close_terminals();
        let exe_path = env::current_exe().unwrap();
        let exe_str = match exe_path.to_str() {
            Some(s) => s,
            None => {
                error!("Failed to convert executable path to string");
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

        let _ = self.open_terminal_with_logs();

        exit(0);
    }
}
