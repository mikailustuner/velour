//! Offline downloads via yt-dlp (low priority), indexed in a JSON file.

use crate::settings::{state_dir, write_json};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use velour_ytm::Track;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub track: Track,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub entries: BTreeMap<String, Entry>,
}

impl Index {
    fn path() -> PathBuf {
        state_dir().join("downloads.json")
    }

    /// Loads the index and drops entries whose files were deleted outside the app.
    pub fn load() -> Self {
        let mut idx: Self =
            std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        idx.entries.retain(|_, e| e.path.exists());
        idx
    }

    pub fn save(&self) {
        write_json(&Self::path(), self);
    }

    pub fn file(&self, video_id: &str) -> Option<&Path> {
        self.entries.get(video_id).map(|e| e.path.as_path()).filter(|p| p.exists())
    }

    pub fn tracks(&self) -> Vec<Track> {
        self.entries.values().map(|e| e.track.clone()).collect()
    }

    pub fn remove(&mut self, video_id: &str) -> Result<()> {
        if let Some(e) = self.entries.remove(video_id)
            && e.path.exists()
        {
            std::fs::remove_file(&e.path).with_context(|| format!("deleting {}", e.path.display()))?;
        }
        self.save();
        Ok(())
    }
}

fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    cleaned.trim().trim_matches('.').chars().take(120).collect()
}

/// Blocking: downloads one track as Opus with tags and cover. Returns the file path.
pub fn download(yt_dlp: &Path, cookies_arg: &str, dir: &Path, track: &Track) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let artist = track.artists.first().map(|a| a.name.as_str()).unwrap_or("Unknown artist");
    let stem = safe_name(&format!("{artist} - {}", track.title));
    let template = dir.join(format!("{stem} [{}].%(ext)s", track.video_id));
    let out = Command::new("nice")
        .args(["-n", "15"])
        .arg(yt_dlp)
        .args(["-q", "--no-warnings", "--no-playlist", "-f", "774/251/bestaudio", "-x", "--audio-format", "opus"])
        .args([
            "--embed-metadata",
            "--embed-thumbnail",
            "--convert-thumbnails",
            "jpg",
            "--cookies-from-browser",
            cookies_arg,
        ])
        .arg("-o")
        .arg(&template)
        .arg("--print")
        .arg("after_move:filepath")
        .arg(format!("https://music.youtube.com/watch?v={}", track.video_id))
        .output()
        .context("starting yt-dlp")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "{}",
            err.lines().rfind(|l| l.contains("ERROR")).unwrap_or("download failed").trim_start_matches("ERROR: ")
        );
    }
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or("").trim());
    if !path.exists() {
        bail!("yt-dlp did not report the downloaded file");
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn safe_names() {
        assert_eq!(super::safe_name("AC/DC: Back?"), "AC_DC_ Back_");
        assert_eq!(super::safe_name("  ..hidden "), "hidden");
    }
}
