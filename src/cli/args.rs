use clap::Parser;

use crate::config::Config;

#[derive(Parser, Debug)]
#[command(
    name = "chaturbate-recorder",
    about = "Record live video streams from Chaturbate",
    version
)]
pub struct Args {
    /// Room(s) to record. Can be specified multiple times.
    #[arg(short, long = "room", value_name = "ROOM")]
    pub rooms: Vec<String>,

    /// Output directory for recordings
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<String>,

    /// Monitor mode - wait for rooms to come online and auto-record
    #[arg(short, long)]
    pub monitor: bool,

    /// Target video resolution (e.g., 1080, 720, 480)
    #[arg(long, value_name = "HEIGHT")]
    pub resolution: Option<u32>,

    /// Target framerate (30 or 60)
    #[arg(long, value_name = "FPS")]
    pub fps: Option<u32>,

    /// Cookies for private streams (semicolon-separated)
    #[arg(long, value_name = "COOKIES", env = "CB_COOKIES")]
    pub cookies: Option<String>,

    /// Custom User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,

    /// Maximum recording duration in minutes (0 = unlimited)
    #[arg(long, value_name = "MINUTES")]
    pub max_duration: Option<u32>,

    /// Maximum file size in MB (0 = unlimited)
    #[arg(long, value_name = "MB")]
    pub max_filesize: Option<u32>,

    /// Check interval in seconds for monitor mode
    #[arg(long, value_name = "SECONDS")]
    pub check_interval: Option<u64>,

    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    pub config: String,

    /// Quiet mode - minimal output
    #[arg(short, long)]
    pub quiet: bool,

    /// Enable debug logging
    #[arg(long)]
    pub debug: bool,
}

impl Args {
    pub fn merge_into_config(&self, config: &mut Config) {
        // Merge rooms from CLI and config
        if !self.rooms.is_empty() {
            config.monitor.rooms = self.rooms.clone();
        }

        // Override output directory
        if let Some(ref output) = self.output {
            config.recording.output_directory = output.clone();
        }

        // Override resolution
        if let Some(resolution) = self.resolution {
            config.recording.resolution = resolution;
        }

        // Override framerate
        if let Some(fps) = self.fps {
            config.recording.framerate = fps;
        }

        // Override cookies
        if let Some(ref cookies) = self.cookies {
            config.network.cookies = Some(cookies.clone());
        }

        // Override user agent
        if let Some(ref ua) = self.user_agent {
            config.network.user_agent = Some(ua.clone());
        }

        // Override max duration
        if let Some(max_duration) = self.max_duration {
            config.recording.max_duration_minutes = max_duration;
        }

        // Override max filesize
        if let Some(max_filesize) = self.max_filesize {
            config.recording.max_filesize_mb = max_filesize;
        }

        // Override check interval
        if let Some(interval) = self.check_interval {
            config.monitor.check_interval_seconds = interval;
        }
    }

    pub fn get_rooms(&self, config: &Config) -> Vec<String> {
        if !self.rooms.is_empty() {
            self.rooms.clone()
        } else {
            config.monitor.rooms.clone()
        }
    }
}
