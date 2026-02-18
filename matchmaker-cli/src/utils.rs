use crate::clap::BINARY_FULL;
use cli_boilerplate_automation::{
    bait::ResultExt,
    bog::{self, BogOkExt},
};
use std::{fs::OpenOptions, path::Path};

pub fn init_logger([q, v]: [u8; 2], log_path: &Path) {
    bog::init_bogger(true, true);
    bog::init_filter((3 + v).saturating_sub(q));

    let rust_log = std::env::var("RUST_LOG").ok().map(|val| val.to_lowercase());

    let mut builder = env_logger::Builder::from_default_env();

    if rust_log.is_none() {
        #[cfg(debug_assertions)]
        {
            builder
                .filter(None, log::LevelFilter::Info)
                .filter(Some(BINARY_FULL), log::LevelFilter::Debug);
        }
        #[cfg(not(debug_assertions))]
        {
            builder
                .format_module_path(false)
                .format_target(false)
                .format_timestamp(None);

            let level = cli_boilerplate_automation::bother::level_filter::from_qv(q, v);

            builder.filter(Some(BINARY_FULL), level);
        }
    }

    if let Some(log_file) = OpenOptions::new()
        .truncate(true)
        .write(true)
        .create(true)
        .open(log_path)
        .prefix("Failed to open log file")
        ._wbog()
    {
        builder.target(env_logger::Target::Pipe(Box::new(log_file)));
    }

    builder.init();
}
