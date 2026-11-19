//! Lyrics: synced lines from LRCLIB when available, plain text from YouTube Music otherwise.

use serde::Deserialize;
use sonora_ytm::Track;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Lyrics {
    /// (start seconds, line)
    Synced(Vec<(f64, String)>),
    Plain(String),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricsResult {
    pub lyrics: Lyrics,
    pub source: Option<String>,
}

#[derive(Deserialize)]
struct LrclibHit {
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
}

/// Parses `[mm:ss.xx] text` lines; lines with several stamps are repeated.
pub fn parse_lrc(lrc: &str) -> Vec<(f64, String)> {
    let mut out = vec![];
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut stamps = vec![];
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            match parse_stamp(tag) {
                Some(t) => stamps.push(t),
                None => {
                    // Metadata tag like [ar:Artist]: skip the whole line.
                    stamps.clear();
                    rest = "";
                    break;
                }
            }
            rest = &stripped[end + 1..];
        }
        for t in stamps {
            out.push((t, rest.trim().to_string()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn parse_stamp(tag: &str) -> Option<f64> {
    let (m, s) = tag.split_once(':')?;
    let m: f64 = m.parse().ok()?;
    let s: f64 = s.parse().ok()?;
    Some(m * 60.0 + s)
}

/// Index of the line active at `pos` seconds.
pub fn active_line(lines: &[(f64, String)], pos: f64) -> Option<usize> {
    let i = lines.partition_point(|(t, _)| *t <= pos + 0.15);
    i.checked_sub(1)
}

pub async fn fetch(http: &reqwest::Client, ytm: &sonora_ytm::Client, track: &Track) -> LyricsResult {
    if let Some(r) = fetch_lrclib(http, track).await {
        return r;
    }
    match ytm.lyrics(&track.video_id).await {
        Ok(Some(l)) => LyricsResult { lyrics: Lyrics::Plain(l.text), source: l.source },
        _ => LyricsResult { lyrics: Lyrics::None, source: None },
    }
}

async fn fetch_lrclib(http: &reqwest::Client, track: &Track) -> Option<LyricsResult> {
    let artist = track.artists.first()?.name.clone();
    let mut req = http
        .get("https://lrclib.net/api/get")
        .timeout(Duration::from_secs(8))
        .header("User-Agent", "Sonora/0.1 (Linux desktop music player)")
        .query(&[("track_name", track.title.as_str()), ("artist_name", artist.as_str())]);
    if let Some(album) = &track.album {
        req = req.query(&[("album_name", album.name.as_str())]);
    }
    if let Some(d) = track.duration {
        req = req.query(&[("duration", d.to_string())]);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let hit: LrclibHit = resp.json().await.ok()?;
    let source = Some("LRCLIB".to_string());
    if let Some(s) = hit.synced.filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(&s);
        if !lines.is_empty() {
            return Some(LyricsResult { lyrics: Lyrics::Synced(lines), source });
        }
    }
    hit.plain.filter(|p| !p.trim().is_empty()).map(|p| LyricsResult { lyrics: Lyrics::Plain(p), source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_with_metadata_and_multi_stamps() {
        let lrc = "[ar:Someone]\n[00:01.50]Hello\n[00:10.00][01:00.25]Chorus\n[00:05.00]\n";
        let l = parse_lrc(lrc);
        assert_eq!(
            l,
            vec![(1.5, "Hello".into()), (5.0, "".into()), (10.0, "Chorus".into()), (60.25, "Chorus".into()),]
        );
    }

    #[test]
    fn active_line_follows_time() {
        let l = vec![(1.0, "a".to_string()), (5.0, "b".to_string())];
        assert_eq!(active_line(&l, 0.5), None);
        assert_eq!(active_line(&l, 1.0), Some(0));
        assert_eq!(active_line(&l, 4.99), Some(1), "small lead so the line lights on the beat");
        assert_eq!(active_line(&l, 100.0), Some(1));
    }
}
