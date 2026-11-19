//! Login via the cookies of the user's Firefox profile.

use anyhow::{Context, Result, bail};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ORIGIN: &str = "https://music.youtube.com";

pub struct Cookies {
    /// `name=value; ...` ready for the Cookie header.
    header: String,
    sapisid: String,
}

impl Cookies {
    /// Reads youtube.com cookies from the default Firefox profile.
    pub fn from_firefox() -> Result<Self> {
        let profile = default_firefox_profile()?;
        Self::from_firefox_profile(&profile)
    }

    pub fn from_firefox_profile(profile: &Path) -> Result<Self> {
        // Firefox keeps the database locked while running, so read a copy.
        let tmp = std::env::temp_dir().join(format!("sonora-cookies-{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;
        let result = (|| {
            for suffix in ["", "-wal"] {
                let src = profile.join(format!("cookies.sqlite{suffix}"));
                if src.exists() {
                    std::fs::copy(&src, tmp.join(format!("cookies.sqlite{suffix}")))?;
                }
            }
            let db = rusqlite::Connection::open(tmp.join("cookies.sqlite"))?;
            let mut stmt = db.prepare(
                "SELECT name, value FROM moz_cookies WHERE host IN ('.youtube.com', 'youtube.com', 'music.youtube.com', '.music.youtube.com') AND originAttributes = ''",
            )?;
            let pairs = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, anyhow::Error>(pairs)
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        let pairs = result.with_context(|| format!("reading cookies from {}", profile.display()))?;

        let sapisid = pairs
            .iter()
            .find(|(n, _)| n == "SAPISID" || n == "__Secure-3PAPISID")
            .map(|(_, v)| v.clone())
            .context("no SAPISID cookie: log in to YouTube in Firefox first")?;
        // ST-* are short-lived page-state cookies; sending them all blows past header limits (413).
        let header = pairs.iter().filter(|(n, _)| !n.starts_with("ST-")).map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ");
        Ok(Self { header, sapisid })
    }

    pub fn header(&self) -> &str {
        &self.header
    }

    /// `Authorization` header value expected by InnerTube for cookie sessions.
    pub fn sapisid_hash(&self) -> String {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let digest = Sha1::digest(format!("{ts} {} {ORIGIN}", self.sapisid));
        format!("SAPISIDHASH {ts}_{}", hex::encode(digest))
    }
}

/// Profile directory Firefox opens by default (also handed to yt-dlp).
pub fn default_firefox_profile() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME not set")?;
    let root = PathBuf::from(home).join(".mozilla/firefox");
    // installs.ini names the profile Firefox actually opens; profiles.ini is the fallback.
    for (file, key) in [("installs.ini", "Default="), ("profiles.ini", "Path=")] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            for line in text.lines() {
                if let Some(rel) = line.strip_prefix(key) {
                    let p = root.join(rel.trim());
                    if p.join("cookies.sqlite").exists() {
                        return Ok(p);
                    }
                }
            }
        }
    }
    bail!("no Firefox profile with cookies found under {}", root.display())
}
