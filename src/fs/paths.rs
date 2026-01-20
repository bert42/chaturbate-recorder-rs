use chrono::Local;
use std::path::PathBuf;

use crate::error::Result;

pub fn generate_output_path(
    output_dir: &str,
    pattern: &str,
    room: &str,
    sequence: u32,
) -> Result<PathBuf> {
    let now = Local::now();

    // Replace template variables
    let filename = pattern
        .replace("{{.Username}}", room)
        .replace("{{.Year}}", &now.format("%Y").to_string())
        .replace("{{.Month}}", &now.format("%m").to_string())
        .replace("{{.Day}}", &now.format("%d").to_string())
        .replace("{{.Hour}}", &now.format("%H").to_string())
        .replace("{{.Minute}}", &now.format("%M").to_string())
        .replace("{{.Second}}", &now.format("%S").to_string());

    // Add sequence suffix if > 0
    let filename = if sequence > 0 {
        format!("{}_{}", filename, sequence)
    } else {
        filename
    };

    // Add .ts extension
    let filename = format!("{}.ts", filename);

    let path = PathBuf::from(output_dir).join(filename);

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_output_path() {
        let path = generate_output_path(
            "./recordings",
            "{{.Username}}_test",
            "testroom",
            0,
        )
        .unwrap();

        assert!(path.to_string_lossy().contains("testroom_test.ts"));
    }

    #[test]
    fn test_generate_output_path_with_sequence() {
        let path = generate_output_path(
            "./recordings",
            "{{.Username}}_test",
            "testroom",
            5,
        )
        .unwrap();

        assert!(path.to_string_lossy().contains("testroom_test_5.ts"));
    }
}
