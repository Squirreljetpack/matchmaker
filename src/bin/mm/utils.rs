use env_logger::Builder;
use log::LevelFilter;
use std::{fs::OpenOptions, path::Path};

pub fn init_logger(file: &Path) {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create parent directories for log file");
    }
    
    let file = OpenOptions::new()
    .create(true)
    .write(true)
    .truncate(true)
    .open(&file)
    .expect("Failed to open log file");
    
    Builder::new()
    .target(env_logger::Target::Pipe(Box::new(file)))
    .filter(None, LevelFilter::Debug)
    .init();
}