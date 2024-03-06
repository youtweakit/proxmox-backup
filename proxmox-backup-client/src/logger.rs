use env_logger::Builder;
use log::LevelFilter;

/// Configure the logger to write to the specified log file
pub fn init_logger() {
    let log_file_path = "/var/log/pbs-client/email.log"; // U can change path if u like

    Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::File(log_file_path.into()))
        .init();
}
