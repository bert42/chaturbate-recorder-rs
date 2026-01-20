# Chaturbate Stream Recorder

A fast, lightweight CLI tool for recording live video streams from Chaturbate, written in Rust.

## Features

- **Direct recording** - Record streams from online rooms immediately
- **Monitor mode** - Watch rooms and automatically start recording when they go online
- **Concurrent recording** - Record multiple rooms simultaneously
- **No FFmpeg required** - Direct MPEG-TS concatenation (output plays in VLC, mpv, etc.)
- **File splitting** - Split recordings by duration or file size
- **Resolution selection** - Choose target resolution and framerate
- **Graceful shutdown** - Ctrl+C finishes current segment cleanly

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/chaturbate-recorder-rs.git
cd chaturbate-recorder-rs

# Build release binary
cargo build --release

# Binary will be at ./target/release/chaturbate-recorder
```

### Requirements

- Rust 1.70+ (for building)
- No runtime dependencies

## Usage

### Basic Recording

```bash
# Record a single room
chaturbate-recorder -r roomname

# Record multiple rooms concurrently
chaturbate-recorder -r room1 -r room2 -r room3

# Specify output directory
chaturbate-recorder -r roomname -o /path/to/recordings
```

### Monitor Mode

Monitor mode watches rooms and automatically starts recording when they come online:

```bash
# Monitor a single room
chaturbate-recorder -r roomname --monitor

# Monitor multiple rooms
chaturbate-recorder -r room1 -r room2 --monitor

# Custom check interval (default: 60 seconds)
chaturbate-recorder -r roomname --monitor --check-interval 30
```

### Quality Settings

```bash
# Select resolution (default: 1080)
chaturbate-recorder -r roomname --resolution 720

# Select framerate (default: 30)
chaturbate-recorder -r roomname --fps 60

# Combined
chaturbate-recorder -r roomname --resolution 1080 --fps 60
```

### File Splitting

```bash
# Split files every 60 minutes
chaturbate-recorder -r roomname --max-duration 60

# Split files at 1GB
chaturbate-recorder -r roomname --max-filesize 1024
```

### Other Options

```bash
# Quiet mode (minimal output)
chaturbate-recorder -r roomname --quiet

# Debug logging
chaturbate-recorder -r roomname --debug

# Use config file
chaturbate-recorder -c /path/to/config.toml

# Show help
chaturbate-recorder --help
```

## Configuration

Create a `config.toml` file (see `config.example.toml`):

```toml
[recording]
output_directory = "./recordings"
filename_pattern = "{{.Username}}_{{.Year}}-{{.Month}}-{{.Day}}_{{.Hour}}-{{.Minute}}-{{.Second}}"
max_duration_minutes = 0    # 0 = unlimited
max_filesize_mb = 0         # 0 = unlimited
resolution = 1080
framerate = 30

[monitor]
check_interval_seconds = 60
rooms = ["room1", "room2"]  # Rooms to monitor

[network]
# user_agent = "Custom User-Agent"
# cookies = "sessionid=abc123"  # For private streams
domain = "https://chaturbate.com/"
```

### Filename Pattern Variables

| Variable | Description |
|----------|-------------|
| `{{.Username}}` | Room name |
| `{{.Year}}` | 4-digit year |
| `{{.Month}}` | 2-digit month |
| `{{.Day}}` | 2-digit day |
| `{{.Hour}}` | 2-digit hour (24h) |
| `{{.Minute}}` | 2-digit minute |
| `{{.Second}}` | 2-digit second |

## Output Format

Recordings are saved as `.ts` (MPEG Transport Stream) files, which:
- Play directly in VLC, mpv, and most media players
- Can be converted to MP4 without re-encoding:

```bash
ffmpeg -i recording.ts -c copy recording.mp4
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CB_COOKIES` | Cookies for private streams |

## Project Structure

```
src/
├── main.rs           # Entry point
├── lib.rs            # Library exports
├── error.rs          # Error types
├── cli/              # CLI argument parsing
├── config/           # Configuration loading
├── api/              # HTTP client
├── stream/           # Stream discovery, recording, monitoring
├── fs/               # File path utilities
└── output/           # Console output, progress bars
```

## License

MIT

## Disclaimer

This tool is for personal use only. Respect content creators' rights and the platform's terms of service. Only record streams you have permission to record.
