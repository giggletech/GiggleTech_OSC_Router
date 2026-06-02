pub const VERSION: &str = "2.1";

pub const APP_NAME: &str = "GiggleTech OSC Router";

pub fn display_name() -> String {
    format!("{APP_NAME} v{VERSION}")
}
