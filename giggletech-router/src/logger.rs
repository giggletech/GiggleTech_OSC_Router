use log::LevelFilter;
use log4rs::{
    append::console::ConsoleAppender,
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
    filter::threshold::ThresholdFilter,
};
use std::env;
use std::path::PathBuf;

pub fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration for logging to a file
    let binding = env::current_exe()?;
    let current_exe_dir = binding
        .parent()
        .ok_or("Could not determine the executable's directory")?;
    let log_file_path: PathBuf = current_exe_dir.join("output.log");
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}{n}")))
        .build(log_file_path)?;

    // Configuration for logging to the console
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{m}{n}")))
        .build();

    // Build the logging configuration
    let config = Config::builder()
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(LevelFilter::Error)))
                .build("logfile", Box::new(logfile)),
        )
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(LevelFilter::Info)))
                .build("stdout", Box::new(stdout)),
        )
        .build(
            Root::builder()
                .appender("logfile")
                .appender("stdout")
                .build(LevelFilter::Info),
        )?;

    log4rs::init_config(config)?;
    Ok(())
}
