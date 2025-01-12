use std::{fs::File, io::Read};

use serde::{Deserialize, Serialize};
use toml;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub interval_seconds: u64,
    pub hour_offset: i64,
    pub access_key: String,
    pub schedule_file: String,
    pub status_file: String,
}

impl Config {
    pub fn initialize() -> Self {
        let mut file = File::open(".config.toml").expect("No .config.toml file found");
        let mut buff = String::new();
        file.read_to_string(&mut buff)
            .expect("Couldn't read .config.toml to buffer");
        let config: Config = toml::from_str(&buff).expect("Couldn't create config from buffer");
        config
    }
}
