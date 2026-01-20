use regex::Regex;

use crate::error::{Error, Result};

pub fn validate_room_name(room: &str) -> Result<()> {
    if room.is_empty() {
        return Err(Error::InvalidRoomName(
            "Room name cannot be empty".to_string(),
        ));
    }

    // Chaturbate room names are alphanumeric with underscores
    let re = Regex::new(r"^[a-zA-Z0-9_]+$")?;
    if !re.is_match(room) {
        return Err(Error::InvalidRoomName(format!(
            "Room name '{}' contains invalid characters. Only letters, numbers, and underscores are allowed.",
            room
        )));
    }

    if room.len() > 50 {
        return Err(Error::InvalidRoomName(format!(
            "Room name '{}' is too long (max 50 characters)",
            room
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_room_names() {
        assert!(validate_room_name("testroom").is_ok());
        assert!(validate_room_name("test_room").is_ok());
        assert!(validate_room_name("TestRoom123").is_ok());
        assert!(validate_room_name("a").is_ok());
    }

    #[test]
    fn test_invalid_room_names() {
        assert!(validate_room_name("").is_err());
        assert!(validate_room_name("test-room").is_err());
        assert!(validate_room_name("test room").is_err());
        assert!(validate_room_name("test.room").is_err());
    }
}
