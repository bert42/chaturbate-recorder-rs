use chrono::Local;
use console::style;

fn timestamp() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn print_banner() {
    let banner = r#"
╔═══════════════════════════════════════════════════════╗
║           Chaturbate Stream Recorder                  ║
╚═══════════════════════════════════════════════════════╝
"#;
    println!("{}", style(banner).cyan());
}

pub fn print_info(message: &str) {
    println!("{} {} {}", timestamp(), style("INFO").cyan().bold(), message);
}

pub fn print_success(message: &str) {
    println!("{} {} {}", timestamp(), style("OK").green().bold(), message);
}

pub fn print_warning(message: &str) {
    println!("{} {} {}", timestamp(), style("WARN").yellow().bold(), message);
}

pub fn print_error(message: &str) {
    eprintln!("{} {} {}", timestamp(), style("ERROR").red().bold(), message);
}

pub fn print_recording(room: &str, message: &str) {
    println!(
        "{} {} [{}] {}",
        timestamp(),
        style("REC").red().bold(),
        style(room).cyan(),
        message
    );
}
