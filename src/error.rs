use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Room not found: {0}")]
    RoomNotFound(String),

    #[error("Broadcaster offline: {0}")]
    BroadcasterOffline(String),

    #[error("Stream URL not found for room: {0}")]
    StreamNotFound(String),

    #[error("Cloudflare blocked request - cookies expired or User-Agent mismatch. Refresh cf_clearance cookie.")]
    CloudflareBlocked,

    #[error("Age verification required")]
    AgeVerification,

    #[error("Private stream - authentication required (need valid sessionid cookie)")]
    PrivateStream,

    #[error("Recording interrupted")]
    Interrupted,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("No rooms specified")]
    NoRoomsSpecified,

    #[error("Invalid room name: {0}")]
    InvalidRoomName(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("M3U8 parse error: {0}")]
    M3u8(String),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Segment download failed after retries: {0}")]
    SegmentDownloadFailed(String),
}

pub type Result<T> = std::result::Result<T, Error>;

// Exit codes
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_CONFIG_ERROR: i32 = 1;
pub const EXIT_NETWORK_ERROR: i32 = 2;
pub const EXIT_RECORDING_ERROR: i32 = 3;
pub const EXIT_INTERRUPTED: i32 = 130;

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Config(_) | Error::InvalidRoomName(_) | Error::NoRoomsSpecified => {
                EXIT_CONFIG_ERROR
            }
            Error::Network(_) | Error::CloudflareBlocked | Error::AgeVerification => {
                EXIT_NETWORK_ERROR
            }
            Error::Interrupted => EXIT_INTERRUPTED,
            _ => EXIT_RECORDING_ERROR,
        }
    }
}
