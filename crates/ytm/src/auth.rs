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
        let tmp = std::env::temp_dir().join(format!("velour-cookies-{}", std::process::id()));
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

        Self::from_pairs(pairs)
    }

    /// Browsers yt-dlp can read cookies from (besides the native Firefox path).
    pub const BROWSERS: [&'static str; 7] = ["firefox", "chrome", "chromium", "brave", "edge", "vivaldi", "opera"];

    /// Reads cookies from `browser`. Firefox is read directly; Chromium-based browsers
    /// encrypt cookies, so yt-dlp (which knows how to decrypt them) exports them to a
    /// private temp file that is deleted right after reading.
    pub fn from_browser(browser: &str, yt_dlp: &Path) -> Result<Self> {
        if browser == "firefox" {
            return Self::from_firefox();
        }
        let dir = std::env::temp_dir().join(format!("velour-cookies-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let file = dir.join("cookies.txt");
        let run = std::process::Command::new(yt_dlp)
            .args([
                "-q",
                "--no-warnings",
                "--skip-download",
                "--ignore-errors",
                "--cookies-from-browser",
                browser,
                "--cookies",
            ])
            .arg(&file)
            // Any URL works: yt-dlp writes the cookie jar on exit. This one never resolves.
            .arg("https://cookies.invalid/")
            .output();
        let text = std::fs::read_to_string(&file);
        let _ = std::fs::remove_dir_all(&dir);
        run.context("starting yt-dlp to read browser cookies")?;
        let text = text.with_context(|| format!("couldn't read {browser} cookies (is it installed and signed in?)"))?;
        Self::from_netscape(&text)
    }

    /// Parses a Netscape cookie file, keeping youtube.com cookies.
    pub fn from_netscape(text: &str) -> Result<Self> {
        let mut pairs: Vec<(String, String)> = vec![];
        for line in text.lines() {
            // "#HttpOnly_" prefixed lines are real cookies.
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                continue;
            }
            let (domain, name, value) = (f[0], f[5], f[6]);
            let ok_domain =
                matches!(domain, ".youtube.com" | "youtube.com" | "music.youtube.com" | ".music.youtube.com");
            if ok_domain && !pairs.iter().any(|(n, _)| n == name) {
                pairs.push((name.to_string(), value.to_string()));
            }
        }
        Self::from_pairs(pairs)
    }

    fn from_pairs(pairs: Vec<(String, String)>) -> Result<Self> {
        let sapisid = pairs
            .iter()
            .find(|(n, _)| n == "SAPISID" || n == "__Secure-3PAPISID")
            .map(|(_, v)| v.clone())
            .context("not signed in: log in to music.youtube.com in your browser first")?;
        // ST-* are short-lived page-state cookies; sending them all blows past header limits (413).
        let header = pairs
            .iter()
            .filter(|(n, _)| !n.starts_with("ST-"))
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netscape_parsing() {
        let text = "# Netscape HTTP Cookie File\n\
.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n\
#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tsid1\n\
.youtube.com\tTRUE\t/\tTRUE\t0\tST-xyz\tjunk\n\
.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tduplicate\n\
.google.com\tTRUE\t/\tTRUE\t0\tNID\tother\n";
        let c = Cookies::from_netscape(text).unwrap();
        assert_eq!(c.header(), "SAPISID=abc; SID=sid1");
        assert!(c.sapisid_hash().starts_with("SAPISIDHASH "));
    }

    #[test]
    fn signed_out_is_an_error() {
        assert!(Cookies::from_netscape(".youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n").is_err());
    }
}
