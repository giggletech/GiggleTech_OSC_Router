use crate::path;
use log::LevelFilter;
use log4rs::{
    append::console::ConsoleAppender,
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
    filter::threshold::ThresholdFilter,
};
use std::fs::OpenOptions;
use std::io;
use std::{ffi::CString, path::PathBuf};

#[cfg(target_os = "windows")]
use winapi::um::fileapi::SetFileAttributesA;
#[cfg(target_os = "windows")]
use winapi::um::winnt::FILE_ATTRIBUTE_HIDDEN;

pub const LOG_FILE: &str = "output.log";
const ERROR_LOG_FILE: &str = "errors.log";

pub fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    // Reset the "output.log" file so that we have a clean log every time the application starts
    let log_file_path: PathBuf = path::join_exe_dir_with_file(LOG_FILE)?;
    let _ = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_file_path)?;

    // Make the file hidden on Windows
    #[cfg(target_os = "windows")]
    set_file_hidden(&log_file_path)?;

    // Configuration for logging everything to a file
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{m}{n}")))
        .build(log_file_path)?;

    // Configuration for logging errors to a file
    let error_log_file_path = path::join_exe_dir_with_file(ERROR_LOG_FILE)?;
    let error_logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}{n}")))
        .build(error_log_file_path)?;

    // Configuration for logging to the console
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{m}{n}")))
        .build();

    // Build the logging configuration
    let config = Config::builder()
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(LevelFilter::Error)))
                .build("error_logfile", Box::new(error_logfile)),
        )
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(LevelFilter::Info)))
                .build("logfile", Box::new(logfile)),
        )
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(LevelFilter::Info)))
                .build("stdout", Box::new(stdout)),
        )
        .build(
            Root::builder()
                .appender("error_logfile")
                .appender("logfile")
                .appender("stdout")
                .build(LevelFilter::Info),
        )?;

    log4rs::init_config(config)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_file_hidden(path: &PathBuf) -> io::Result<()> {
    let log_file_path_str = path.to_str().ok_or(io::Error::new(
        io::ErrorKind::Other,
        "Path contains invalid Unicode characters",
    ))?;

    let path_c = CString::new(log_file_path_str).unwrap();
    unsafe {
        if SetFileAttributesA(path_c.as_ptr() as *const i8, FILE_ATTRIBUTE_HIDDEN) == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
