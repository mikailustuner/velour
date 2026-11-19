//! User settings and saved session, stored as JSON under XDG config/state dirs.

use crate::queue::PlayQueue;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback))
        .join("velour")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache")
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub volume: f64,
    /// Keep playing in the background when the window is closed.
    pub close_to_background: bool,
    /// Desktop notification on track change (needs a notification daemon).
    pub notifications: bool,
    /// System tray icon (needs a StatusNotifier host such as waybar).
    pub tray: bool,
    /// Discord Rich Presence; needs a Discord application id.
    pub discord: bool,
    pub discord_app_id: String,
    pub eq_preset: String,
    pub download_dir: PathBuf,
    /// Record plays in the YouTube Music history (improves recommendations).
    pub send_history: bool,
    /// List the user's playlists in the sidebar (off is handy when sharing the screen).
    pub sidebar_playlists: bool,
    /// Offline downloads are opt-in (personal use only; see README).
    pub downloads: bool,
    /// Browser to take the YouTube Music sign-in from.
    pub browser: String,
    /// Software rendering: ~160 MB less memory but far more CPU while scrolling.
    pub low_memory: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let music = std::env::var_os("XDG_MUSIC_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Music"));
        Self {
            theme: Theme::Dark,
            volume: 70.0,
            close_to_background: true,
            notifications: false,
            tray: false,
            discord: false,
            discord_app_id: String::new(),
            eq_preset: "Flat".into(),
            download_dir: music.join("Velour"),
            send_history: true,
            low_memory: false,
            browser: "firefox".into(),
            downloads: false,
            sidebar_playlists: true,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        write_json(&Self::path(), self);
    }
}

/// What was playing when the app quit.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub queue: PlayQueue,
    pub position: f64,
}

impl Session {
    fn path() -> PathBuf {
        state_dir().join("session.json")
    }

    pub fn load() -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()
    }

    pub fn save(&self) {
        write_json(&Self::path(), self);
    }
}

/// Atomic write (temp file + rename) so a crash never leaves half a file.
pub fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    let result = (|| -> anyhow::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("velour: could not save {}: {e:#}", path.display());
    }
}
