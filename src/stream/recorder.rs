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
            Ok(content) => content,
            Err(e) => {
                tracing::warn!("Failed to fetch playlist for {}: {}", stream_info.room, e);
                // Could be temporary network issue, wait and retry
                tokio::time::sleep(poll_interval).await;
                continue;
            }
        };

        // Parse media playlist
        let playlist = match m3u8_rs::parse_media_playlist_res(playlist_content.as_bytes()) {
            Ok(pl) => pl,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse media playlist for {}: {:?}",
                    stream_info.room,
                    e
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

        // Process segments
        for segment in &playlist.segments {
            if let Some(seq) = tracker.extract_sequence(&segment.uri) {
                if tracker.is_new_segment(seq) {
                    // Download segment
                    let segment_url = resolve_segment_url(&stream_info.hls_source, &segment.uri)?;

                    match download_segment_with_retry(client, &segment_url, 3).await {
                        Ok(data) => {
                            // Write to output file
                            output_file.write_all(&data).await?;

                            let bytes = data.len() as u64;
                            let duration = segment.duration as f64;
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
