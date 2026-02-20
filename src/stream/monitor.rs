use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    Private,
    Recording,
    CookieDead,
}

struct ActiveRecording {
    handle: JoinHandle<Result<RecordingStats>>,
    cancel_token: CancellationToken,
}

/// Tracks per-room check state for backoff and dedup
struct RoomCheckState {
    /// Last error type seen (for dedup — only log on change)
    last_error_kind: Option<RoomErrorKind>,
    /// How many consecutive checks returned the same error
    consecutive_same_error: u32,
    /// Next allowed check time (for backoff)
    next_check_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
enum RoomErrorKind {
    Offline,
    Private,
    ServerError,
    Cloudflare,
    Other,
}

impl RoomCheckState {
    fn new() -> Self {
        Self {
            last_error_kind: None,
            consecutive_same_error: 0,
            next_check_at: None,
        }
    }

    /// Record an error and return whether this is a NEW error (should be logged)
    fn record_error(&mut self, kind: RoomErrorKind, base_interval: Duration) -> bool {
        let is_new = self.last_error_kind.as_ref() != Some(&kind);

        if is_new {
            self.last_error_kind = Some(kind);
            self.consecutive_same_error = 1;
            // Reset backoff on new error type
            self.next_check_at = Some(Instant::now() + base_interval);
            true
        } else {
            self.consecutive_same_error += 1;
            // Exponential backoff: base * 2^min(consecutive, 6) — max ~64x interval
            let multiplier = 2u32.pow(self.consecutive_same_error.min(6));
            self.next_check_at = Some(Instant::now() + base_interval * multiplier);
            false
        }
    }

    /// Record a success — resets all backoff/dedup state
    fn record_success(&mut self) {
        self.last_error_kind = None;
        self.consecutive_same_error = 0;
        self.next_check_at = None;
    }

    /// Should we skip this room's check due to backoff?
    fn should_skip(&self) -> bool {
        self.next_check_at
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }
}

pub struct RoomMonitor {
    client: Arc<ChaturbateClient>,
    rooms: Vec<String>,
    check_interval: Duration,
    recording_config: RecordingConfig,
    room_status: Arc<RwLock<HashMap<String, RoomStatus>>>,
    webhook_url: Option<String>,
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
            webhook_url: monitor_config.webhook_url.clone(),
        }
    }

    pub async fn run(&self, cancel_token: CancellationToken) -> Result<()> {
        let mut active_recordings: HashMap<String, ActiveRecording> = HashMap::new();
        let mut check_states: HashMap<String, RoomCheckState> = HashMap::new();
        let mut cookie_dead = false;
        let mut cookie_dead_alerted = false;

        for room in &self.rooms {
            check_states.insert(room.clone(), RoomCheckState::new());
        }

        console::print_info(&format!(
            "Monitor mode started for {} room(s). Checking every {}s.",
            self.rooms.len(),
            self.check_interval.as_secs()
        ));

        if self.webhook_url.is_some() {
            console::print_info("Webhook notifications enabled.");
        }

        loop {
            if cancel_token.is_cancelled() {
                console::print_info("Shutting down monitor...");

                for (room, recording) in active_recordings.iter() {
                    console::print_info(&format!("Stopping recording for {}...", room));
                    recording.cancel_token.cancel();
                }

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

            // --- Check all rooms and collect results ---
            let mut private_count: u32 = 0;
            let mut cloudflare_count: u32 = 0;
            let mut checked_count: u32 = 0;

            for room in &self.rooms {
                let is_recording = active_recordings.contains_key(room);
                let check_state = check_states.entry(room.clone()).or_insert_with(RoomCheckState::new);

                // Skip rooms in backoff (unless cookie was just fixed)
                if !cookie_dead && check_state.should_skip() {
                    continue;
                }

                checked_count += 1;

                match self.check_room(room).await {
                    Ok(stream_info) if !is_recording => {
                        // Room is online — start recording
                        console::print_success(&format!(
                            "{} is ONLINE at {}p{}fps - starting recording",
                            room, stream_info.resolution, stream_info.framerate
                        ));

                        check_state.record_success();

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
                    Ok(_) => {
                        // Room online but already recording — nothing to do
                        check_state.record_success();
                    }
                    Err(Error::BroadcasterOffline(_)) => {
                        if !is_recording {
                            let is_new = check_state.record_error(RoomErrorKind::Offline, self.check_interval);
                            if is_new {
                                console::print_info(&format!("{} is offline", room));
                            }
                            self.set_status(room, RoomStatus::Offline).await;
                        }
                    }
                    Err(Error::PrivateStream) => {
                        private_count += 1;
                        if !is_recording {
                            let is_new = check_state.record_error(RoomErrorKind::Private, self.check_interval);
                            if is_new {
                                console::print_info(&format!("{} is private", room));
                            }
                            self.set_status(room, RoomStatus::Private).await;
                        }
                    }
                    Err(Error::CloudflareBlocked) => {
                        cloudflare_count += 1;
                        if !is_recording {
                            let is_new = check_state.record_error(RoomErrorKind::Cloudflare, self.check_interval);
                            if is_new {
                                console::print_error(&format!("{}: Cloudflare blocked", room));
                            }
                        }
                    }
                    Err(Error::ServerError(status, ref msg)) => {
                        if !is_recording {
                            let is_new = check_state.record_error(RoomErrorKind::ServerError, self.check_interval);
                            if is_new {
                                console::print_error(&format!("{}: Server error {} - {}", room, status, msg));
                            }
                        }
                    }
                    Err(e) => {
                        let is_new = check_state.record_error(RoomErrorKind::Other, self.check_interval);
                        if is_new {
                            console::print_error(&format!("{}: {}", room, e));
                        }
                    }
                }
            }

            // --- Global cookie death detection ---
            // If >50% of checked rooms return Private or Cloudflare, cookies are dead
            let auth_fail_count = private_count + cloudflare_count;
            let was_cookie_dead = cookie_dead;

            if checked_count > 0 && auth_fail_count > 0 && auth_fail_count * 2 >= checked_count {
                if !cookie_dead {
                    cookie_dead = true;
                    cookie_dead_alerted = false;

                    console::print_error(&format!(
                        "🍪 COOKIE DEATH DETECTED — {}/{} rooms returning private/cloudflare. All checks paused with backoff.",
                        auth_fail_count, checked_count
                    ));

                    // Set all non-recording rooms to CookieDead
                    for room in &self.rooms {
                        if !active_recordings.contains_key(room) {
                            self.set_status(room, RoomStatus::CookieDead).await;
                        }
                    }
                }

                // Send webhook alert (once per cookie death event)
                if !cookie_dead_alerted {
                    self.send_webhook("🍪 Cookie died! All rooms returning private/cloudflare. Fix: solve CAPTCHA and update cf_clearance cookie.").await;
                    cookie_dead_alerted = true;
                }
            } else if cookie_dead && auth_fail_count == 0 && checked_count > 0 {
                // Cookie is working again!
                cookie_dead = false;
                cookie_dead_alerted = false;

                console::print_success("🍪 Cookie recovered! Rooms responding normally again.");
                self.send_webhook("🍪 Cookie recovered! Recorder is back to normal.").await;

                // Reset all backoff states so rooms get checked immediately
                for state in check_states.values_mut() {
                    state.record_success();
                }
            }

            // --- Clean up finished recordings ---
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

    /// Send a webhook notification (fire-and-forget)
    async fn send_webhook(&self, message: &str) {
        let url = match &self.webhook_url {
            Some(url) => url.clone(),
            None => return,
        };

        let payload = serde_json::json!({
            "text": message,
            "source": "chaturbate-recorder",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let client = reqwest::Client::new();
        match client.post(&url).json(&payload).timeout(Duration::from_secs(10)).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Webhook sent successfully");
            }
            Ok(resp) => {
                tracing::warn!("Webhook returned {}: {}", resp.status(), url);
            }
            Err(e) => {
                tracing::warn!("Webhook failed: {}", e);
            }
        }
    }
}
