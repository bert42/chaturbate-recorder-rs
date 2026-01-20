use console::style;

use crate::stream::RecordingStats;

pub fn print_recording_stats(room: &str, stats: &RecordingStats) {
    println!("{}", style("═".repeat(50)).dim());
    println!("Recording stats for {}:", style(room).cyan().bold());
    println!("  Segments:    {}", stats.segments_downloaded);
    println!(
        "  Total size:  {:.2} MB",
        stats.bytes_written as f64 / 1024.0 / 1024.0
    );
    println!("  Duration:    {}", format_duration(stats.duration_seconds));
    println!("  Files:       {}", stats.files_created);
    println!("{}", style("═".repeat(50)).dim());
}

pub fn print_summary(total_rooms: usize, successful: usize, failed: usize) {
    println!();
    println!("{}", style("═".repeat(50)).dim());
    println!("Session Summary:");
    println!("  Total rooms:  {}", total_rooms);
    println!(
        "  Successful:   {}",
        style(successful.to_string()).green()
    );
    if failed > 0 {
        println!("  Failed:       {}", style(failed.to_string()).red());
    }
    println!("{}", style("═".repeat(50)).dim());
}

fn format_duration(seconds: f64) -> String {
    let total_secs = seconds as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}
