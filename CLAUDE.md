# CLAUDE.md - Project Guide for AI Assistants

## Project Overview

Chaturbate Recorder RS is a Rust CLI application that records live video streams from Chaturbate. It captures HLS streams and saves them as MPEG-TS files without requiring FFmpeg.

## Build Commands

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Check without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy

# Run the application
./target/release/chaturbate-recorder -r <roomname>
```

## Project Structure

```
chaturbate-recorder-rs/
├── Cargo.toml              # Dependencies and project metadata
├── config.example.toml     # Example configuration file
├── src/
│   ├── main.rs             # Entry point, CLI setup, mode dispatch
│   ├── lib.rs              # Library root, public exports
│   ├── error.rs            # Error types (thiserror), exit codes
│   ├── cli/
│   │   ├── mod.rs
│   │   └── args.rs         # Clap CLI argument definitions
│   ├── config/
│   │   ├── mod.rs
│   │   ├── loader.rs       # Config struct, TOML parsing
│   │   └── validation.rs   # Room name validation
│   ├── api/
│   │   ├── mod.rs
│   │   └── client.rs       # HTTP client with headers/cookies
│   ├── stream/
│   │   ├── mod.rs
│   │   ├── discovery.rs    # HLS URL extraction from room page
│   │   ├── recorder.rs     # Main recording loop
│   │   ├── segment.rs      # Segment tracking and download
│   │   └── monitor.rs      # Monitor mode (auto-record)
│   ├── fs/
│   │   ├── mod.rs
│   │   └── paths.rs        # Output path generation
│   └── output/
│       ├── mod.rs
│       ├── console.rs      # Colored output (console crate)
│       ├── progress.rs     # Progress bars (indicatif)
│       └── stats.rs        # Recording statistics
```

## Architecture Notes

### Stream Discovery Flow

1. Fetch room page at `https://chaturbate.com/{room}/`
2. Check for `playlist.m3u8` presence (indicates online status)
3. Extract `window.initialRoomDossier` JSON via regex
4. Decode unicode escapes, parse JSON for `hls_source` URL
5. Fetch master playlist, select variant by resolution/framerate

### Recording Flow

1. Poll media playlist every 1 second
2. Track segment sequence numbers (regex: `_(\d+)\.ts$`)
3. Download new segments with retry (3 attempts, 600ms delay)
4. Write directly to output `.ts` file (no temp storage)
5. Split file on max duration/size thresholds
6. Handle `#EXT-X-ENDLIST` for stream termination

### Key Design Decisions

- **No FFmpeg**: MPEG-TS segments concatenate directly
- **Gzip/Deflate**: reqwest handles compressed responses automatically
- **Graceful shutdown**: CancellationToken propagates Ctrl+C
- **Concurrent rooms**: Each room runs in separate tokio task

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime with signal handling |
| `reqwest` | HTTP client with gzip/deflate support |
| `clap` | CLI argument parsing with derive |
| `serde` + `toml` | Config file parsing |
| `m3u8-rs` | HLS playlist parsing |
| `tracing` | Structured logging |
| `indicatif` | Progress bars |
| `console` | Terminal colors |
| `thiserror` | Error type definitions |
| `chrono` | Timestamp formatting |
| `regex` | Stream URL and segment extraction |

## Common Tasks

### Adding CLI Arguments

1. Add field to `Args` struct in `cli/args.rs`
2. Add merge logic in `Args::merge_into_config()`
3. Use in `main.rs` or pass through config

### Adding New Stream Source

1. Create new discovery function in `stream/discovery.rs`
2. Adapt `RoomDossier` struct for new JSON format
3. Update `select_variant()` if playlist format differs

### Modifying Output Filename

Edit `generate_output_path()` in `fs/paths.rs`. Template variables:
- `{{.Username}}`, `{{.Year}}`, `{{.Month}}`, `{{.Day}}`
- `{{.Hour}}`, `{{.Minute}}`, `{{.Second}}`

### Adding Error Types

1. Add variant to `Error` enum in `error.rs`
2. Update `exit_code()` match if needed
3. Use `#[from]` for automatic conversion

## Testing

```bash
# Test with offline room (should show "broadcaster offline")
./target/release/chaturbate-recorder -r nonexistent_room_12345

# Test with online room (record for a few seconds, Ctrl+C)
./target/release/chaturbate-recorder -r <online_room>

# Verify output plays
vlc recordings/*.ts
# Or convert to mp4
ffmpeg -i recording.ts -c copy recording.mp4
```

## Configuration

Copy `config.example.toml` to `config.toml`:

```toml
[recording]
output_directory = "./recordings"
resolution = 1080
framerate = 30

[monitor]
check_interval_seconds = 60
rooms = []

[network]
# cookies = "sessionid=abc"  # For private streams
domain = "https://chaturbate.com/"
```

CLI arguments override config file values.
