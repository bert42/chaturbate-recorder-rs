use regex::Regex;
use serde::Deserialize;

use crate::api::ChaturbateClient;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub hls_source: String,
    pub room: String,
    pub resolution: u32,
    pub framerate: u32,
}

#[derive(Debug, Deserialize)]
struct RoomDossier {
    hls_source: Option<String>,
}

pub async fn get_stream_info(
    client: &ChaturbateClient,
    room: &str,
    target_resolution: u32,
    target_framerate: u32,
) -> Result<StreamInfo> {
    // Fetch room page
    let html = client.get_room_page(room).await?;

    // Check if online (has playlist)
    if !html.contains("playlist.m3u8") {
        return Err(Error::BroadcasterOffline(room.to_string()));
    }

    // Extract initialRoomDossier JSON
    let re = Regex::new(r#"window\.initialRoomDossier\s*=\s*"(.+?)""#)?;
    let captures = re
        .captures(&html)
        .ok_or_else(|| Error::StreamNotFound(room.to_string()))?;
    let encoded_json = &captures[1];

    // Decode unicode escapes
    let json_str = decode_unicode_escapes(encoded_json)?;

    // Parse JSON to get hls_source
    let dossier: RoomDossier = serde_json::from_str(&json_str)?;
    let master_url = dossier
        .hls_source
        .ok_or_else(|| Error::StreamNotFound(room.to_string()))?;

    if master_url.is_empty() {
        return Err(Error::BroadcasterOffline(room.to_string()));
    }

    // Fetch master playlist and select variant
    let (playlist_url, resolution, framerate) =
        select_variant(client, &master_url, target_resolution, target_framerate).await?;

    Ok(StreamInfo {
        hls_source: playlist_url,
        room: room.to_string(),
        resolution,
        framerate,
    })
}

fn decode_unicode_escapes(input: &str) -> Result<String> {
    let mut result = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('u') => {
                    chars.next(); // consume 'u'
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        }
                    }
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('r') => {
                    chars.next();
                    result.push('\r');
                }
                Some('t') => {
                    chars.next();
                    result.push('\t');
                }
                Some('"') => {
                    chars.next();
                    result.push('"');
                }
                Some('\\') => {
                    chars.next();
                    result.push('\\');
                }
                Some('/') => {
                    chars.next();
                    result.push('/');
                }
                _ => {
                    result.push(c);
                }
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

#[derive(Debug)]
struct Variant {
    url: String,
    resolution: u32,
    framerate: u32,
    bandwidth: u64,
}

async fn select_variant(
    client: &ChaturbateClient,
    master_url: &str,
    target_resolution: u32,
    target_framerate: u32,
) -> Result<(String, u32, u32)> {
    let content = client.get(master_url).await?;

    // Parse master playlist
    let playlist = m3u8_rs::parse_master_playlist_res(content.as_bytes())
        .map_err(|e| Error::M3u8(format!("Failed to parse master playlist: {:?}", e)))?;

    let mut variants: Vec<Variant> = Vec::new();

    for variant in &playlist.variants {
        let resolution = variant
            .resolution
            .as_ref()
            .map(|r| r.height as u32)
            .unwrap_or(0);

        // Detect framerate from NAME or other attributes
        // Chaturbate uses "FPS:60.0" in the NAME field for 60fps streams
        let framerate = if variant
            .other_attributes
            .as_ref()
            .and_then(|attrs| attrs.get("NAME"))
            .map(|name| name.to_string().contains("FPS:60"))
            .unwrap_or(false)
        {
            60
        } else {
            30
        };

        let url = resolve_url(master_url, &variant.uri)?;

        variants.push(Variant {
            url,
            resolution,
            framerate,
            bandwidth: variant.bandwidth,
        });
    }

    if variants.is_empty() {
        return Err(Error::M3u8("No variants found in master playlist".to_string()));
    }

    // Sort by resolution (descending), then framerate (descending), then bandwidth (descending)
    variants.sort_by(|a, b| {
        b.resolution
            .cmp(&a.resolution)
            .then(b.framerate.cmp(&a.framerate))
            .then(b.bandwidth.cmp(&a.bandwidth))
    });

    // Find best match: exact resolution and framerate, or highest below target
    let selected = variants
        .iter()
        .find(|v| v.resolution == target_resolution && v.framerate == target_framerate)
        .or_else(|| {
            variants
                .iter()
                .find(|v| v.resolution <= target_resolution && v.framerate <= target_framerate)
        })
        .unwrap_or(&variants[0]);

    Ok((
        selected.url.clone(),
        selected.resolution,
        selected.framerate,
    ))
}

fn resolve_url(base: &str, path: &str) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }

    let base_url = url::Url::parse(base)?;
    let resolved = base_url.join(path)?;
    Ok(resolved.to_string())
}

pub fn resolve_segment_url(playlist_url: &str, segment_uri: &str) -> Result<String> {
    resolve_url(playlist_url, segment_uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_unicode_escapes() {
        let input = r#"hello\u0020world"#;
        let result = decode_unicode_escapes(input).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_decode_unicode_escapes_quotes() {
        let input = r#"test\"value\""#;
        let result = decode_unicode_escapes(input).unwrap();
        assert_eq!(result, r#"test"value""#);
    }
}
