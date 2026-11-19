//! Turns a YouTube video id into a direct audio stream URL using yt-dlp.
//! yt-dlp runs at low CPU priority; results are cached until the URL is about to expire.

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Premium 256k Opus, Premium 256k AAC, then the best free formats.
pub const FORMAT: &str = "774/141/251/140/bestaudio";
const TIMEOUT: Duration = Duration::from_secs(60);

pub struct Resolver {
    yt_dlp: PathBuf,
    /// Value for `--cookies-from-browser` (e.g. `firefox:/path/to/profile` or `brave`).
    cookies: Mutex<String>,
    cache: Mutex<HashMap<String, (String, u64)>>,
    /// Ids being resolved right now; a second request for the same id waits instead of
    /// starting another yt-dlp (prefetch and play often overlap).
    inflight: Mutex<HashSet<String>>,
    done: Condvar,
}

impl Resolver {
    /// `cookies_from` is a `--cookies-from-browser` value, e.g. `firefox:/path` or `chrome`.
    pub fn new(cookies_from: String) -> Self {
        Self {
            yt_dlp: find_yt_dlp(),
            cookies: Mutex::new(cookies_from),
            cache: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashSet::new()),
            done: Condvar::new(),
        }
    }

    pub fn yt_dlp(&self) -> &PathBuf {
        &self.yt_dlp
    }

    pub fn cookies_arg(&self) -> String {
        self.cookies.lock().unwrap().clone()
    }

    /// Switches the browser used for cookies; cached URLs stay valid.
    pub fn set_cookies_arg(&self, value: String) {
        *self.cookies.lock().unwrap() = value;
    }

    /// Cached URL if it is still valid for a while.
    pub fn cached(&self, video_id: &str) -> Option<String> {
        let (url, expire) = self.cache.lock().unwrap().get(video_id)?.clone();
        (expire > now_secs() + 600).then_some(url)
    }

    /// Blocking; call from a worker thread.
    pub fn resolve(&self, video_id: &str) -> Result<String> {
        {
            let mut inflight = self.inflight.lock().unwrap();
            loop {
                if let Some(url) = self.cached(video_id) {
                    return Ok(url);
                }
                if !inflight.contains(video_id) {
                    inflight.insert(video_id.to_string());
                    break;
                }
                inflight = self.done.wait(inflight).unwrap();
            }
        }
        let result = self.resolve_uncached(video_id);
        self.inflight.lock().unwrap().remove(video_id);
        self.done.notify_all();
        result
    }

    /// True while `video_id` is being resolved.
    pub fn is_resolving(&self, video_id: &str) -> bool {
        self.inflight.lock().unwrap().contains(video_id)
    }

    fn resolve_uncached(&self, video_id: &str) -> Result<String> {
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

/// State of the external helpers playback depends on.
#[derive(Debug, Clone)]
pub struct Health {
    /// e.g. "2026.10.26"; None when yt-dlp is missing.
    pub yt_dlp_version: Option<String>,
    /// Days since that release.
    pub yt_dlp_age_days: Option<i64>,
    pub js_runtime: bool,
    /// Self-updating binary (`yt-dlp -U` works) vs. a distro package.
    pub updatable: bool,
}

impl Resolver {
    /// Blocking: runs `yt-dlp --version` and looks for a JavaScript runtime.
    pub fn health(&self) -> Health {
        let yt_dlp_version = Command::new(&self.yt_dlp)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let yt_dlp_age_days = yt_dlp_version.as_deref().and_then(release_age_days);
        let js_runtime = ["deno", "node", "bun", "qjs"].iter().any(|b| in_path(b));
        let updatable = std::fs::metadata(&self.yt_dlp).map(|m| !m.permissions().readonly()).unwrap_or(false)
            && self.yt_dlp.is_absolute()
            && !self.yt_dlp.starts_with("/usr");
        Health { yt_dlp_version, yt_dlp_age_days, js_runtime, updatable }
    }

    /// Blocking: `yt-dlp -U`. Returns yt-dlp's last output line.
    pub fn update_yt_dlp(&self) -> Result<String> {
        let out = Command::new(&self.yt_dlp).arg("-U").output().context("starting yt-dlp")?;
        let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).to_string();
        let last = text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        if !out.status.success() {
            bail!("{last}");
        }
        Ok(last)
    }
}

/// "2026.10.26" → days since that date.
fn release_age_days(version: &str) -> Option<i64> {
    let mut parts = version.split('.').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let days = days_from_civil(y, m, d);
    let today = (now_secs() / 86_400) as i64;
    Some(today - days)
}

/// Days since 1970-01-01 for a Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn in_path(bin: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let extra = [home.join(".local/bin"), home.join(".deno/bin"), home.join(".bun/bin")];
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra)
        .any(|dir| dir.join(bin).is_file())
}

/// True for yt-dlp errors that a newer yt-dlp usually fixes.
pub fn looks_outdated(error: &str) -> bool {
    let e = error.to_lowercase();
    ["sign in to confirm", "challenge", "needs to be reloaded", "requested format is not available", "nsig", "player response"]
        .iter()
        .any(|k| e.contains(k))
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
    fn release_age() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert!(release_age_days("2026.10.26").is_some());
        assert_eq!(release_age_days("nightly"), None);
    }

    #[test]
    fn outdated_hints() {
        assert!(looks_outdated("ERROR: [youtube] x: The page needs to be reloaded."));
        assert!(!looks_outdated("Video unavailable. This video is private"));
    }

    #[test]
    fn parses_expire() {
        let u = "https://rr1.googlevideo.com/videoplayback?expire=1763600000&ei=x&itag=774";
        assert_eq!(query_param(u, "expire"), Some("1763600000"));
        assert_eq!(query_param(u, "itag"), Some("774"));
        assert_eq!(query_param(u, "nope"), None);
    }
}
