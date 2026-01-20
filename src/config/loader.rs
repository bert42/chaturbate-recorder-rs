use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub network: NetworkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    #[serde(default = "default_output_directory")]
    pub output_directory: String,
    #[serde(default = "default_filename_pattern")]
    pub filename_pattern: String,
    #[serde(default)]
    pub max_duration_minutes: u32,
    #[serde(default)]
    pub max_filesize_mb: u32,
    #[serde(default = "default_resolution")]
    pub resolution: u32,
    #[serde(default = "default_framerate")]
    pub framerate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_check_interval")]
    pub check_interval_seconds: u64,
    #[serde(default)]
    pub rooms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default = "default_domain")]
    pub domain: String,
}

fn default_output_directory() -> String {
    "./recordings".to_string()
}

fn default_filename_pattern() -> String {
    "{{.Username}}_{{.Year}}-{{.Month}}-{{.Day}}_{{.Hour}}-{{.Minute}}-{{.Second}}".to_string()
}

fn default_resolution() -> u32 {
    1080
}

fn default_framerate() -> u32 {
    30
}

fn default_check_interval() -> u64 {
    60
}

fn default_domain() -> String {
    "https://chaturbate.com/".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            recording: RecordingConfig::default(),
            monitor: MonitorConfig::default(),
            network: NetworkConfig::default(),
        }
    }
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            output_directory: default_output_directory(),
            filename_pattern: default_filename_pattern(),
            max_duration_minutes: 0,
            max_filesize_mb: 0,
            resolution: default_resolution(),
            framerate: default_framerate(),
        }
    }
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            check_interval_seconds: default_check_interval(),
            rooms: Vec::new(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            user_agent: None,
            cookies: None,
            domain: default_domain(),
        }
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default() -> Self {
        Self::load("config.toml").unwrap_or_default()
    }
}

impl NetworkConfig {
    pub fn domain_with_trailing_slash(&self) -> String {
        if self.domain.ends_with('/') {
            self.domain.clone()
        } else {
            format!("{}/", self.domain)
        }
    }
}

impl RecordingConfig {
    pub fn poll_interval_ms(&self) -> u64 {
        1000 // Fixed 1 second polling interval
    }
}
