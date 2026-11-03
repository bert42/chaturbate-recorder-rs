use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::api::ChaturbateClient;
use crate::config::RecordingConfig;
use crate::error::Result;
use crate::fs::generate_output_path;
use crate::stream::discovery::resolve_segment_url;
use crate::stream::segment::{download_segment_with_retry, SegmentTracker};
use crate::stream::StreamInfo;

#[derive(Debug, Default)]
pub struct RecordingStats {
    pub segments_downloaded: u64,
    pub bytes_written: u64,
    pub duration_seconds: f64,
    pub files_created: u32,
}

pub async fn record_stream(
    client: &ChaturbateClient,
    stream_info: &StreamInfo,
    config: &RecordingConfig,
    cancel_token: CancellationToken,
) -> Result<RecordingStats> {
    let mut stats = RecordingStats::default();
    let mut tracker = SegmentTracker::new()?;

    // Create initial output file
    let (mut output_file, mut current_path) =
        create_output_file(&stream_info.room, config, 0).await?;
    stats.files_created = 1;

    let mut file_duration: f64 = 0.0;
    let mut file_size: u64 = 0;
    let mut file_sequence: u32 = 0;

    let poll_interval = Duration::from_millis(config.poll_interval_ms());
    let max_duration_secs = (config.max_duration_minutes as f64) * 60.0;
    let max_filesize_bytes = (config.max_filesize_mb as u64) * 1024 * 1024;

    // Track consecutive failures to detect stream becoming unavailable
    let mut consecutive_failures: u32 = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;

    // fMP4 init segment (moov box). Every output file needs it at the start, so it is kept
    // and rewritten after each split.
    let mut init_segment: Option<Vec<u8>> = None;

    // Periodic progress line instead of per-poll logging
    const PROGRESS_INTERVAL: Duration = Duration::from_secs(600);
    let mut last_progress = std::time::Instant::now();

    tracing::info!(
        "Recording {} at {}p{}fps to {}",
        stream_info.room,
        stream_info.resolution,
        stream_info.framerate,
        current_path.display()
    );

    loop {
        // Check for cancellation
        if cancel_token.is_cancelled() {
            tracing::info!("Recording cancelled for {}", stream_info.room);
            break;
        }

        // Fetch media playlist
        let playlist_content = match client.get(&stream_info.hls_source).await {
            Ok(content) => {
                if consecutive_failures > 0 {
                    tracing::info!(
                        "Playlist for {} is back after {} failed attempt(s)",
                        stream_info.room,
                        consecutive_failures
                    );
                }
                consecutive_failures = 0;
                tracing::debug!("Playlist: {} bytes for {}", content.len(), stream_info.room);
                content
            }
            Err(e) => {
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    tracing::info!(
                        "Stream unavailable for {} after {} consecutive failures, stopping recording",
                        stream_info.room,
                        consecutive_failures
                    );
                    break;
                }
                log_retry(
                    consecutive_failures,
                    &format!("Failed to fetch playlist for {}: {}", stream_info.room, e),
                );
                tokio::time::sleep(poll_interval).await;
                continue;
            }
        };

        // Parse media playlist
        let playlist = match m3u8_rs::parse_media_playlist_res(playlist_content.as_bytes()) {
            Ok(pl) => pl,
            Err(e) => {
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    tracing::info!(
                        "Stream unavailable for {} after {} consecutive failures, stopping recording",
                        stream_info.room,
                        consecutive_failures
                    );
                    break;
                }
                log_retry(
                    consecutive_failures,
                    &format!("Failed to parse media playlist for {}: {:?}", stream_info.room, e),
                );
                tokio::time::sleep(poll_interval).await;
                continue;
            }
        };

        // Check for stream end
        if playlist.end_list {
            tracing::info!("Stream ended for {}", stream_info.room);
            break;
        }

        // Download the fMP4 init segment (moov box) once and write it to the first file
        if init_segment.is_none() {
            let map_uri = playlist_content
                .lines()
                .find(|l| l.starts_with("#EXT-X-MAP"))
                .and_then(|l| l.split("URI=").nth(1))
                .map(|s| s.trim_matches('"').to_string());

            if let Some(init_uri) = map_uri {
                match resolve_segment_url(&stream_info.hls_source, &init_uri) {
                    Ok(init_url) => match download_segment_with_retry(client, &init_url, 3).await {
                        Ok(data) => {
                            tracing::debug!("Init segment: {} bytes for {}", data.len(), stream_info.room);
                            output_file.write_all(&data).await?;
                            stats.bytes_written += data.len() as u64;
                            file_size += data.len() as u64;
                            init_segment = Some(data);
                        }
                        Err(e) => tracing::warn!("Init segment failed for {}: {}", stream_info.room, e),
                    },
                    Err(e) => tracing::warn!("Init URL resolve failed for {}: {}", stream_info.room, e),
                }
            }
        }

        // Parse segments from raw LL-HLS playlist text
        let segment_uris: Vec<String> = playlist_content
            .lines()
            .filter(|line| {
                let l = line.trim();
                !l.is_empty() && !l.starts_with("#") && l.contains("/seg_") && l.contains(".m4s")
            })
            .map(|s| s.trim().to_string())
            .collect();
        for uri in &segment_uris {
            if let Some(seq) = tracker.extract_sequence(uri) {
                if tracker.is_new_segment(seq) {
                    // Download segment
                    let segment_url = resolve_segment_url(&stream_info.hls_source, uri)?;

                    match download_segment_with_retry(client, &segment_url, 3).await {
                        Ok(data) => {
                            // Write to output file
                            output_file.write_all(&data).await?;

                            let bytes = data.len() as u64;
                            let duration = 2.0;
                            file_size += bytes;
                            file_duration += duration;
                            stats.bytes_written += bytes;
                            stats.duration_seconds += duration;
                            stats.segments_downloaded += 1;

                            tracker.update_sequence(seq);

                            // Check if we need to split file
                            if should_split_file(
                                file_duration,
                                file_size,
                                max_duration_secs,
                                max_filesize_bytes,
                            ) {
                                output_file.flush().await?;
                                drop(output_file);

                                file_sequence += 1;
                                let (new_file, new_path) = create_output_file(
                                    &stream_info.room,
                                    config,
                                    file_sequence,
                                )
                                .await?;

                                output_file = new_file;
                                current_path = new_path;
                                file_duration = 0.0;
                                file_size = 0;
                                stats.files_created += 1;

                                // Without the init segment the new file would not play
                                if let Some(ref init) = init_segment {
                                    output_file.write_all(init).await?;
                                    stats.bytes_written += init.len() as u64;
                                    file_size += init.len() as u64;
                                }

                                tracing::info!(
                                    "Split recording, new file: {}",
                                    current_path.display()
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to download segment {} for {}: {}",
                                seq,
                                stream_info.room,
                                e
                            );
                        }
                    }
                }
            }
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            last_progress = std::time::Instant::now();
            tracing::info!(
                "{}: recording, {:.0} min, {:.1} MB so far ({})",
                stream_info.room,
                stats.duration_seconds / 60.0,
                stats.bytes_written as f64 / 1024.0 / 1024.0,
                current_path.display()
            );
        }

        // Wait before next poll
        tokio::time::sleep(poll_interval).await;
    }

    // Flush and close file
    output_file.flush().await?;

    tracing::info!(
        "Recording complete for {}: {} segments, {:.2} MB, {:.0}s",
        stream_info.room,
        stats.segments_downloaded,
        stats.bytes_written as f64 / 1024.0 / 1024.0,
        stats.duration_seconds
    );

    Ok(stats)
}

/// Warn on the first failure of a streak, keep retries at debug level.
fn log_retry(attempt: u32, message: &str) {
    if attempt == 1 {
        tracing::warn!("{} (will retry)", message);
    } else {
        tracing::debug!("{} (attempt {})", message, attempt);
    }
}

fn should_split_file(
    duration: f64,
    size: u64,
    max_duration_secs: f64,
    max_filesize_bytes: u64,
) -> bool {
    if max_duration_secs > 0.0 && duration >= max_duration_secs {
        return true;
    }
    if max_filesize_bytes > 0 && size >= max_filesize_bytes {
        return true;
    }
    false
}

async fn create_output_file(
    room: &str,
    config: &RecordingConfig,
    sequence: u32,
) -> Result<(File, PathBuf)> {
    let path = generate_output_path(
        &config.output_directory,
        &config.filename_pattern,
        room,
        sequence,
    )?;

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .await?;

    Ok((file, path))
}
