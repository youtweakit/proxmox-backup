// logger.rs

use env_logger::Builder;
use log::{error, info, LevelFilter};

// Path to log file
pub const LOG_FILE_PATH: &str = "/var/log/pbs-client/email.log";

/// Configure the logger to write to the log file
pub fn init_logger() {
    Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::File(LOG_FILE_PATH.into()))
        .init();
}

//logging errors
pub fn log_error(message: &str, error: &dyn std::error::Error) {
    error!("{}: {}", message, error);
}
