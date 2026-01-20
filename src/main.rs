use clap::Parser;
use std::process::ExitCode;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use chaturbate_recorder::api::ChaturbateClient;
use chaturbate_recorder::cli::Args;
use chaturbate_recorder::config::{validate_room_name, Config};
use chaturbate_recorder::error::{Error, EXIT_SUCCESS};
use chaturbate_recorder::output::console;
use chaturbate_recorder::stream::{get_stream_info, record_stream, RoomMonitor};

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    // Setup logging
    let filter = if args.debug {
        EnvFilter::new("debug")
    } else if args.quiet {
        EnvFilter::new("error")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Load and merge config
    let mut config = Config::load(&args.config).unwrap_or_else(|e| {
        console::print_warning(&format!("Failed to load config: {}. Using defaults.", e));
        Config::default()
    });

    args.merge_into_config(&mut config);

    // Get rooms to record
    let rooms = args.get_rooms(&config);

    if rooms.is_empty() {
        console::print_error("No rooms specified. Use -r <room> or configure rooms in config.toml");
        return ExitCode::from(1);
    }

    // Validate room names
    for room in &rooms {
        if let Err(e) = validate_room_name(room) {
            console::print_error(&format!("{}", e));
            return ExitCode::from(1);
        }
    }

    // Create HTTP client
    let client = match ChaturbateClient::new(&config.network) {
        Ok(c) => c,
        Err(e) => {
            console::print_error(&format!("Failed to create HTTP client: {}", e));
            return ExitCode::from(1);
        }
    };

    // Setup cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    // Handle Ctrl+C
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        console::print_info("Received interrupt signal, shutting down...");
        cancel_token_clone.cancel();
    });

    if !args.quiet {
        console::print_banner();
    }

    // Run in monitor mode or direct recording mode
    let result = if args.monitor {
        run_monitor_mode(client, rooms, &config, cancel_token).await
    } else {
        run_direct_mode(client, rooms, &config, cancel_token).await
    };

    match result {
        Ok(_) => ExitCode::from(EXIT_SUCCESS as u8),
        Err(e) => {
            console::print_error(&format!("{}", e));
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

async fn run_monitor_mode(
    client: ChaturbateClient,
    rooms: Vec<String>,
    config: &Config,
    cancel_token: CancellationToken,
) -> Result<(), Error> {
    let monitor = RoomMonitor::new(
        client,
        rooms,
        &config.monitor,
        config.recording.clone(),
    );

    monitor.run(cancel_token).await
}

async fn run_direct_mode(
    client: ChaturbateClient,
    rooms: Vec<String>,
    config: &Config,
    cancel_token: CancellationToken,
) -> Result<(), Error> {
    use std::sync::Arc;
    use tokio::task::JoinSet;

    let client = Arc::new(client);
    let mut tasks: JoinSet<(String, Result<chaturbate_recorder::stream::RecordingStats, Error>)> =
        JoinSet::new();

    // Start recording tasks for each room
    for room in rooms {
        let client = Arc::clone(&client);
        let recording_config = config.recording.clone();
        let cancel_token = cancel_token.clone();

        tasks.spawn(async move {
            console::print_info(&format!("Checking {}...", room));

            // Get stream info
            let stream_info = match get_stream_info(
                &client,
                &room,
                recording_config.resolution,
                recording_config.framerate,
            )
            .await
            {
                Ok(info) => info,
                Err(e) => {
                    return (room, Err(e));
                }
            };

            console::print_success(&format!(
                "{} is online at {}p{}fps",
                room, stream_info.resolution, stream_info.framerate
            ));

            // Start recording
            let result = record_stream(&client, &stream_info, &recording_config, cancel_token).await;

            (room, result)
        });
    }

    let mut successful = 0;
    let mut failed = 0;

    // Wait for all tasks to complete
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((room, Ok(stats))) => {
                chaturbate_recorder::output::stats::print_recording_stats(&room, &stats);
                successful += 1;
            }
            Ok((room, Err(e))) => {
                console::print_error(&format!("{}: {}", room, e));
                failed += 1;
            }
            Err(e) => {
                console::print_error(&format!("Task error: {}", e));
                failed += 1;
            }
        }
    }

    if !cancel_token.is_cancelled() {
        chaturbate_recorder::output::stats::print_summary(successful + failed, successful, failed);
    }

    if failed > 0 && successful == 0 {
        Err(Error::Config("All recordings failed".to_string()))
    } else {
        Ok(())
    }
}
