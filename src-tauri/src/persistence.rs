use crate::innertube::TrackItem;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Lightweight player session persisted to `~/.local/share/com.ytmusic.rust/player_session.json`
/// with strict `0600` file permissions on Unix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub volume: f32,
    pub repeat_mode: String,
    pub autoplay_radio: bool,
    pub low_ram_no_images: bool,
    pub theme: String,
    pub queue: Vec<TrackItem>,
    pub queue_index: usize,
}

impl Default for PersistedSession {
    fn default() -> Self {
        Self {
            volume: 0.8,
            repeat_mode: "off".to_string(),
            autoplay_radio: true,
            low_ram_no_images: false,
            theme: "dark".to_string(),
            queue: Vec::new(),
            queue_index: 0,
        }
    }
}

/// Resolves `~/.local/share/com.ytmusic.rust/player_session.json`.
pub fn get_session_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("com.ytmusic.rust")
        .join("player_session.json")
}

/// Loads the persisted player session from `~/.local/share/com.ytmusic.rust/player_session.json` if present and valid.
pub fn load_persisted_session() -> Option<PersistedSession> {
    let path = get_session_file_path();
    let contents = fs::read_to_string(&path).ok()?;
    let mut session = serde_json::from_str::<PersistedSession>(&contents).ok()?;

    session.volume = session.volume.clamp(0.0, 1.0);
    session.repeat_mode = match session.repeat_mode.trim().to_lowercase().as_str() {
        "all" => "all".to_string(),
        "one" => "one".to_string(),
        _ => "off".to_string(),
    };
    session.theme = match session.theme.trim().to_lowercase().as_str() {
        "light" => "light".to_string(),
        _ => "dark".to_string(),
    };
    if !session.queue.is_empty() && session.queue_index >= session.queue.len() {
        session.queue_index = 0;
    }

    Some(session)
}

/// Saves `PersistedSession` to `~/.local/share/com.ytmusic.rust/player_session.json` with `0700` directory
/// and `0600` file permissions on Unix.
pub fn save_persisted_session(session: &PersistedSession) {
    let path = get_session_file_path();
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let Ok(serialized) = serde_json::to_string_pretty(session) else {
        return;
    };

    if fs::write(&path, serialized).is_err() {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
}
