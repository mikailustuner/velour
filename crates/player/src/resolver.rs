//! Turns a YouTube video id into a direct audio stream URL using yt-dlp.
//! yt-dlp runs at low CPU priority; results are cached until the URL is about to expire.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Premium 256k Opus, Premium 256k AAC, then the best free formats.
pub const FORMAT: &str = "774/141/251/140/bestaudio";
const TIMEOUT: Duration = Duration::from_secs(60);

pub struct Resolver {
    yt_dlp: PathBuf,
    firefox_profile: PathBuf,
    cache: Mutex<HashMap<String, (String, u64)>>,
}

impl Resolver {
    pub fn new(firefox_profile: PathBuf) -> Self {
        Self { yt_dlp: find_yt_dlp(), firefox_profile, cache: Mutex::new(HashMap::new()) }
    }

    pub fn yt_dlp(&self) -> &PathBuf {
        &self.yt_dlp
    }

    pub fn cookies_arg(&self) -> String {
        format!("firefox:{}", self.firefox_profile.display())
    }

    /// Cached URL if it is still valid for a while.
    pub fn cached(&self, video_id: &str) -> Option<String> {
        let (url, expire) = self.cache.lock().unwrap().get(video_id)?.clone();
        (expire > now_secs() + 600).then_some(url)
    }

    /// Blocking; call from a worker thread.
    pub fn resolve(&self, video_id: &str) -> Result<String> {
        if let Some(url) = self.cached(video_id) {
            return Ok(url);
        }
        // The web_music client without page/config fetches is ~40% faster; fall back to
        // yt-dlp's defaults if YouTube changes something under it.
        let url = self
            .run(video_id, Some("youtube:player_client=web_music;player_skip=webpage,configs"))
            .or_else(|_| self.run(video_id, None))?;
        let expire = query_param(&url, "expire").and_then(|e| e.parse().ok()).unwrap_or(now_secs() + 3600);
        self.cache.lock().unwrap().insert(video_id.to_string(), (url.clone(), expire));
        Ok(url)
    }

    /// Cache entries worth persisting across restarts.
    pub fn export_cache(&self) -> Vec<(String, String, u64)> {
        let now = now_secs();
        self.cache.lock().unwrap().iter().filter(|(_, (_, e))| *e > now + 600).map(|(k, (u, e))| (k.clone(), u.clone(), *e)).collect()
    }

    pub fn import_cache(&self, entries: Vec<(String, String, u64)>) {
        let mut c = self.cache.lock().unwrap();
        for (k, u, e) in entries {
            c.insert(k, (u, e));
        }
    }

    fn run(&self, video_id: &str, extractor_args: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("nice");
        cmd.args(["-n", "10"])
            .arg(&self.yt_dlp)
            .args(["-q", "--no-warnings", "--no-playlist", "-g", "-f", FORMAT, "--cookies-from-browser"])
            .arg(self.cookies_arg());
        if let Some(a) = extractor_args {
            cmd.args(["--extractor-args", a]);
        }
        let mut child = cmd
            .arg(format!("https://music.youtube.com/watch?v={video_id}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.yt_dlp.display()))?;

        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if started.elapsed() > TIMEOUT {
                let _ = child.kill();
                bail!("yt-dlp timed out for {video_id}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rfind(|l| l.contains("ERROR")).unwrap_or(err.trim());
            bail!("{}", line.trim_start_matches("ERROR: ").trim());
        }
        let url = String::from_utf8(out.stdout)?.lines().next().unwrap_or("").trim().to_string();
        if !url.starts_with("http") {
            bail!("yt-dlp returned no URL for {video_id}");
        }
        Ok(url)
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// Prefer the self-updating binary in ~/.local/bin over a distro package.
fn find_yt_dlp() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let local = PathBuf::from(home).join(".local/bin/yt-dlp");
        if local.exists() {
            return local;
        }
    }
    PathBuf::from("yt-dlp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_expire() {
        let u = "https://rr1.googlevideo.com/videoplayback?expire=1763600000&ei=x&itag=774";
        assert_eq!(query_param(u, "expire"), Some("1763600000"));
        assert_eq!(query_param(u, "itag"), Some("774"));
        assert_eq!(query_param(u, "nope"), None);
    }
}
