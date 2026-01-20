use console::style;

pub fn print_banner() {
    let banner = r#"
╔═══════════════════════════════════════════════════════╗
║           Chaturbate Stream Recorder                  ║
╚═══════════════════════════════════════════════════════╝
"#;
    println!("{}", style(banner).cyan());
}

pub fn print_info(message: &str) {
    println!("{} {}", style("INFO").cyan().bold(), message);
}

pub fn print_success(message: &str) {
    println!("{} {}", style("OK").green().bold(), message);
}

pub fn print_warning(message: &str) {
    println!("{} {}", style("WARN").yellow().bold(), message);
}

pub fn print_error(message: &str) {
    eprintln!("{} {}", style("ERROR").red().bold(), message);
}

pub fn print_recording(room: &str, message: &str) {
    println!(
        "{} [{}] {}",
        style("REC").red().bold(),
        style(room).cyan(),
        message
    );
}
