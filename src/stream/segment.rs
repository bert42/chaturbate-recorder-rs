use regex::Regex;
use std::time::Duration;

use crate::api::ChaturbateClient;
use crate::error::{Error, Result};

pub struct SegmentTracker {
    last_sequence: u64,
    sequence_regex: Regex,
}

impl SegmentTracker {
    pub fn new() -> Result<Self> {
        Ok(Self {
            last_sequence: 0,
            sequence_regex: Regex::new(r"_(\d+)\.ts$")?,
        })
    }

    pub fn extract_sequence(&self, uri: &str) -> Option<u64> {
        self.sequence_regex
            .captures(uri)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse().ok())
    }

    pub fn is_new_segment(&self, sequence: u64) -> bool {
        sequence > self.last_sequence
    }

    pub fn update_sequence(&mut self, sequence: u64) {
        if sequence > self.last_sequence {
            self.last_sequence = sequence;
        }
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

impl Default for SegmentTracker {
    fn default() -> Self {
        Self::new().expect("Failed to create SegmentTracker")
    }
}

pub async fn download_segment_with_retry(
    client: &ChaturbateClient,
    url: &str,
    max_retries: u32,
) -> Result<Vec<u8>> {
    let mut last_error = None;
    let delay = Duration::from_millis(600);

    for attempt in 0..max_retries {
        match client.get_bytes(url).await {
            Ok(data) => return Ok(data),
            Err(e) => {
                last_error = Some(e);
                if attempt + 1 < max_retries {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        Error::SegmentDownloadFailed(format!("Failed after {} attempts: {}", max_retries, url))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sequence() {
        let tracker = SegmentTracker::new().unwrap();

        assert_eq!(
            tracker.extract_sequence("playlist_480p_123.ts"),
            Some(123)
        );
        assert_eq!(
            tracker.extract_sequence("chunklist_720p30fps_456.ts"),
            Some(456)
        );
        assert_eq!(tracker.extract_sequence("invalid.m3u8"), None);
    }

    #[test]
    fn test_segment_tracker() {
        let mut tracker = SegmentTracker::new().unwrap();

        assert!(tracker.is_new_segment(1));
        tracker.update_sequence(1);
        assert!(!tracker.is_new_segment(1));
        assert!(tracker.is_new_segment(2));
    }
}
