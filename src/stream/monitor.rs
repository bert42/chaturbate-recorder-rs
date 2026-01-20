use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::ChaturbateClient;
use crate::config::{MonitorConfig, RecordingConfig};
use crate::error::{Error, Result};
use crate::output::console;
use crate::stream::discovery::get_stream_info;
use crate::stream::recorder::{record_stream, RecordingStats};

#[derive(Debug, Clone, PartialEq)]
pub enum RoomStatus {
    Unknown,
    Offline,
    Online,
    Recording,
}

struct ActiveRecording {
    handle: JoinHandle<Result<RecordingStats>>,
    cancel_token: CancellationToken,
}

pub struct RoomMonitor {
    client: Arc<ChaturbateClient>,
    rooms: Vec<String>,
    check_interval: Duration,
    recording_config: RecordingConfig,
    room_status: Arc<RwLock<HashMap<String, RoomStatus>>>,
}

impl RoomMonitor {
    pub fn new(
        client: ChaturbateClient,
        rooms: Vec<String>,
        monitor_config: &MonitorConfig,
        recording_config: RecordingConfig,
    ) -> Self {
        let mut initial_status = HashMap::new();
        for room in &rooms {
            initial_status.insert(room.clone(), RoomStatus::Unknown);
        }

        Self {
            client: Arc::new(client),
            rooms,
            check_interval: Duration::from_secs(monitor_config.check_interval_seconds),
            recording_config,
            room_status: Arc::new(RwLock::new(initial_status)),
        }
    }

    pub async fn run(&self, cancel_token: CancellationToken) -> Result<()> {
        let mut active_recordings: HashMap<String, ActiveRecording> = HashMap::new();

        console::print_info(&format!(
            "Monitor mode started for {} room(s). Checking every {}s.",
            self.rooms.len(),
            self.check_interval.as_secs()
        ));

        loop {
            if cancel_token.is_cancelled() {
                console::print_info("Shutting down monitor...");

                // Cancel all active recordings
                for (room, recording) in active_recordings.iter() {
                    console::print_info(&format!("Stopping recording for {}...", room));
                    recording.cancel_token.cancel();
                }

                // Wait for recordings to finish
                for (room, recording) in active_recordings.drain() {
                    match recording.handle.await {
                        Ok(Ok(stats)) => {
                            console::print_success(&format!(
                                "{}: {} segments, {:.2} MB recorded",
                                room,
                                stats.segments_downloaded,
                                stats.bytes_written as f64 / 1024.0 / 1024.0
                            ));
                        }
                        Ok(Err(e)) => {
                            console::print_error(&format!("{}: Recording error: {}", room, e));
                        }
                        Err(e) => {
                            console::print_error(&format!("{}: Task error: {}", room, e));
                        }
                    }
                }

                break;
            }

            // Check each room
            for room in &self.rooms {
                let is_recording = active_recordings.contains_key(room);

                match self.check_room(room).await {
                    Ok(stream_info) if !is_recording => {
                        // Room is online and not recording - start recording
                        console::print_success(&format!(
                            "{} is ONLINE at {}p{}fps - starting recording",
                            room, stream_info.resolution, stream_info.framerate
                        ));

                        let recording_cancel = CancellationToken::new();
                        let handle = self.spawn_recording(
                            room.clone(),
                            stream_info,
                            recording_cancel.clone(),
                        );

                        active_recordings.insert(
                            room.clone(),
                            ActiveRecording {
                                handle,
                                cancel_token: recording_cancel,
                            },
                        );

                        self.set_status(room, RoomStatus::Recording).await;
                    }
                    Err(Error::BroadcasterOffline(_)) => {
                        if !is_recording {
                            // Only log status change
                            let current = self.get_status(room).await;
                            if current != RoomStatus::Offline {
                                console::print_info(&format!("{} is offline", room));
                                self.set_status(room, RoomStatus::Offline).await;
                            }
                        }
                    }
                    Err(e) => {
                        console::print_error(&format!("{}: {}", room, e));
                    }
                    _ => {}
                }
            }

            // Clean up finished recordings
            let mut finished = Vec::new();
            for (room, recording) in active_recordings.iter() {
                if recording.handle.is_finished() {
                    finished.push(room.clone());
                }
            }

            for room in finished {
                if let Some(recording) = active_recordings.remove(&room) {
                    match recording.handle.await {
                        Ok(Ok(stats)) => {
                            console::print_success(&format!(
                                "{}: Recording finished - {} segments, {:.2} MB",
                                room,
                                stats.segments_downloaded,
                                stats.bytes_written as f64 / 1024.0 / 1024.0
                            ));
                        }
                        Ok(Err(e)) => {
                            console::print_error(&format!("{}: Recording error: {}", room, e));
                        }
                        Err(e) => {
                            console::print_error(&format!("{}: Task error: {}", room, e));
                        }
                    }
                    self.set_status(&room, RoomStatus::Unknown).await;
                }
            }

            // Wait before next check
            tokio::select! {
                _ = tokio::time::sleep(self.check_interval) => {}
                _ = cancel_token.cancelled() => {}
            }
        }

        Ok(())
    }

    async fn check_room(
        &self,
        room: &str,
    ) -> Result<crate::stream::StreamInfo> {
        get_stream_info(
            &self.client,
            room,
            self.recording_config.resolution,
            self.recording_config.framerate,
        )
        .await
    }

    fn spawn_recording(
        &self,
        _room: String,
        stream_info: crate::stream::StreamInfo,
        cancel_token: CancellationToken,
    ) -> JoinHandle<Result<RecordingStats>> {
        let client = Arc::clone(&self.client);
        let config = self.recording_config.clone();

        tokio::spawn(async move {
            record_stream(&client, &stream_info, &config, cancel_token).await
        })
    }

    async fn get_status(&self, room: &str) -> RoomStatus {
        self.room_status
            .read()
            .await
            .get(room)
            .cloned()
            .unwrap_or(RoomStatus::Unknown)
    }

    async fn set_status(&self, room: &str, status: RoomStatus) {
        self.room_status
            .write()
            .await
            .insert(room.to_string(), status);
    }
}
