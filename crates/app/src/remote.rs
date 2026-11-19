//! Local control socket for other apps on this computer (e.g. Tiphys, a phone remote).
//!
//! - Off by default (Settings → Desktop). Socket `$XDG_RUNTIME_DIR/velour/control.sock`: the directory is 0700 and
//!   ours, the socket 0600, so only the same user can connect.
//! - One request per connection: a single JSON line in, a single JSON line out.
//!   Request `{"v":1,"op":"search","query":"…"}` → `{"v":1,"ok":true,"result":…}` or
//!   `{"v":1,"ok":false,"error":{"code":"…","message":"…"}}`.
//! - The wire types below are camelCase and separate from the cache/session format, so either can change alone.

use crate::lyrics::{Lyrics, LyricsResult};
use crate::queue::Repeat;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use velour_ytm::{Card, Header, Item, Layout, LikeStatus, Link, Page, PageType, Section, Target, Track};

pub const PROTOCOL: u32 = 1;
const MAX_REQUEST: u64 = 2 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Network calls (search, pages) can take a while; the app answers or this runs out.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TRACKS: usize = 2000;
const MAX_TEXT: usize = 300;

// =====================================================================
// wire types

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WLike {
    Like,
    Dislike,
    Indifferent,
}

impl From<LikeStatus> for WLike {
    fn from(l: LikeStatus) -> Self {
        match l {
            LikeStatus::Like => Self::Like,
            LikeStatus::Dislike => Self::Dislike,
            LikeStatus::Indifferent => Self::Indifferent,
        }
    }
}

impl From<WLike> for LikeStatus {
    fn from(l: WLike) -> Self {
        match l {
            WLike::Like => Self::Like,
            WLike::Dislike => Self::Dislike,
            WLike::Indifferent => Self::Indifferent,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WLink {
    pub name: String,
    #[serde(default)]
    pub browse_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WTrack {
    pub video_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<WLink>,
    #[serde(default)]
    pub album: Option<WLink>,
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub set_video_id: Option<String>,
    #[serde(default = "yes")]
    pub playable: bool,
    #[serde(default)]
    pub like: Option<WLike>,
    #[serde(default)]
    pub detail: Option<String>,
}

fn yes() -> bool {
    true
}

fn wlink(l: &Link) -> WLink {
    WLink { name: l.name.clone(), browse_id: l.browse_id.clone() }
}

fn link(l: WLink) -> Link {
    Link { name: l.name, browse_id: l.browse_id }
}

impl From<&Track> for WTrack {
    fn from(t: &Track) -> Self {
        Self {
            video_id: t.video_id.clone(),
            title: t.title.clone(),
            artists: t.artists.iter().map(wlink).collect(),
            album: t.album.as_ref().map(wlink),
            duration: t.duration,
            thumbnail: t.thumbnail.clone(),
            set_video_id: t.set_video_id.clone(),
            playable: t.playable,
            like: t.like.map(Into::into),
            detail: t.detail.clone(),
        }
    }
}

impl From<WTrack> for Track {
    fn from(t: WTrack) -> Self {
        Self {
            video_id: t.video_id,
            title: t.title,
            artists: t.artists.into_iter().map(link).collect(),
            album: t.album.map(link),
            duration: t.duration,
            thumbnail: t.thumbnail,
            set_video_id: t.set_video_id,
            playable: t.playable,
            like: t.like.map(Into::into),
            detail: t.detail,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WPageType {
    Album,
    Playlist,
    Artist,
    Channel,
    Podcast,
    #[serde(other)]
    Other,
}

impl From<&PageType> for WPageType {
    fn from(p: &PageType) -> Self {
        match p {
            PageType::Album => Self::Album,
            PageType::Playlist => Self::Playlist,
            PageType::Artist => Self::Artist,
            PageType::Channel => Self::Channel,
            PageType::Podcast => Self::Podcast,
            PageType::Other => Self::Other,
        }
    }
}

impl From<WPageType> for PageType {
    fn from(p: WPageType) -> Self {
        match p {
            WPageType::Album => Self::Album,
            WPageType::Playlist => Self::Playlist,
            WPageType::Artist => Self::Artist,
            WPageType::Channel => Self::Channel,
            WPageType::Podcast => Self::Podcast,
            WPageType::Other => Self::Other,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum WTarget {
    Browse {
        id: String,
        #[serde(default)]
        params: Option<String>,
        page_type: WPageType,
    },
    Watch {
        #[serde(default)]
        video_id: Option<String>,
        #[serde(default)]
        playlist_id: Option<String>,
        #[serde(default)]
        params: Option<String>,
    },
}

impl From<&Target> for WTarget {
    fn from(t: &Target) -> Self {
        match t {
            Target::Browse { id, params, page_type } => {
                Self::Browse { id: id.clone(), params: params.clone(), page_type: page_type.into() }
            }
            Target::Watch { video_id, playlist_id, params } => {
                Self::Watch { video_id: video_id.clone(), playlist_id: playlist_id.clone(), params: params.clone() }
            }
        }
    }
}

impl From<WTarget> for Target {
    fn from(t: WTarget) -> Self {
        match t {
            WTarget::Browse { id, params, page_type } => Self::Browse { id, params, page_type: page_type.into() },
            WTarget::Watch { video_id, playlist_id, params } => Self::Watch { video_id, playlist_id, params },
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WCard {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
    pub target: WTarget,
    pub round: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WItem {
    Track(WTrack),
    Card(WCard),
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WSection {
    pub title: String,
    /// "list" (songs), "carousel" or "grid" (cards).
    pub layout: &'static str,
    pub items: Vec<WItem>,
    pub more: Option<WTarget>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WHeader {
    pub title: String,
    pub subtitle: String,
    pub second_subtitle: String,
    pub description: String,
    pub thumbnail: Option<String>,
    pub play: Option<WTarget>,
    pub radio: Option<WTarget>,
    pub playlist_id: Option<String>,
    pub round: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WPage {
    pub header: Option<WHeader>,
    pub sections: Vec<WSection>,
    pub continuation: Option<String>,
}

fn wcard(c: &Card) -> WCard {
    WCard {
        title: c.title.clone(),
        subtitle: c.subtitle.clone(),
        thumbnail: c.thumbnail.clone(),
        target: (&c.target).into(),
        round: c.round,
    }
}

fn wheader(h: &Header) -> WHeader {
    WHeader {
        title: h.title.clone(),
        subtitle: h.subtitle.clone(),
        second_subtitle: h.second_subtitle.clone(),
        description: h.description.clone(),
        thumbnail: h.thumbnail.clone(),
        play: h.play.as_ref().map(Into::into),
        radio: h.radio.as_ref().map(Into::into),
        playlist_id: h.playlist_id.clone(),
        round: h.round,
    }
}

fn wsection(s: &Section) -> WSection {
    WSection {
        title: s.title.clone(),
        layout: match s.layout {
            Layout::List => "list",
            Layout::Carousel => "carousel",
            Layout::Grid => "grid",
        },
        items: s
            .items
            .iter()
            .map(|i| match i {
                Item::Track(t) => WItem::Track(t.into()),
                Item::Card(c) => WItem::Card(wcard(c)),
            })
            .collect(),
        more: s.more.as_ref().map(Into::into),
    }
}

pub fn wpage(p: &Page) -> WPage {
    WPage {
        header: p.header.as_ref().map(wheader),
        sections: p.sections.iter().map(wsection).collect(),
        continuation: p.continuation.clone(),
    }
}

pub fn repeat_name(r: Repeat) -> &'static str {
    match r {
        Repeat::Off => "off",
        Repeat::All => "all",
        Repeat::One => "one",
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WRepeat {
    Off,
    All,
    One,
}

impl From<WRepeat> for Repeat {
    fn from(r: WRepeat) -> Self {
        match r {
            WRepeat::Off => Self::Off,
            WRepeat::All => Self::All,
            WRepeat::One => Self::One,
        }
    }
}

pub fn wlyrics(video_id: &str, r: &LyricsResult) -> Value {
    let (kind, lines, text) = match &r.lyrics {
        Lyrics::Synced(lines) => {
            ("synced", lines.iter().map(|(t, l)| json!({ "time": t, "text": l })).collect::<Vec<_>>(), None)
        }
        Lyrics::Plain(t) => ("plain", vec![], Some(t.clone())),
        Lyrics::None => ("none", vec![], None),
    };
    json!({ "videoId": video_id, "kind": kind, "lines": lines, "text": text, "source": r.source })
}

// =====================================================================
// requests

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibraryTab {
    Playlists,
    Albums,
    Artists,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Op {
    // ---- now playing ----
    State,
    Queue,
    Lyrics,
    // ---- controls ----
    Toggle,
    Next,
    Prev,
    Seek {
        seconds: f64,
    },
    Volume {
        percent: f64,
    },
    Mute {
        on: bool,
    },
    Shuffle {
        on: bool,
    },
    Repeat {
        mode: WRepeat,
    },
    // ---- queue ----
    Jump {
        index: usize,
    },
    Remove {
        index: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    Clear,
    PlayNext {
        track: WTrack,
    },
    AddToQueue {
        track: WTrack,
    },
    // ---- play ----
    Play {
        tracks: Vec<WTrack>,
        #[serde(default)]
        start: usize,
        #[serde(default)]
        shuffle: bool,
    },
    PlayTarget {
        target: WTarget,
    },
    Radio {
        video_id: String,
    },
    // ---- browse ----
    Search {
        query: String,
    },
    Suggest {
        query: String,
    },
    Page {
        id: String,
        #[serde(default)]
        params: Option<String>,
    },
    More {
        token: String,
    },
    Home,
    Library {
        tab: LibraryTab,
    },
    Liked,
    History,
    // ---- account ----
    Rate {
        video_id: String,
        like: WLike,
    },
    Playlists,
    AddToPlaylist {
        playlist_id: String,
        video_ids: Vec<String>,
    },
    CreatePlaylist {
        title: String,
        #[serde(default)]
        video_ids: Vec<String>,
    },
}

fn short(field: &str, s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err(format!("{field} is empty"));
    }
    if s.len() > MAX_TEXT {
        return Err(format!("{field} is too long"));
    }
    Ok(())
}

impl Op {
    /// Limits that serde can't express; index bounds are checked against the live queue by the app.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Op::Seek { seconds } if !seconds.is_finite() || *seconds < 0.0 => Err("seconds must be ≥ 0".into()),
            Op::Volume { percent } if !percent.is_finite() || !(0.0..=100.0).contains(percent) => {
                Err("percent must be 0–100".into())
            }
            Op::PlayNext { track } | Op::AddToQueue { track } => short("videoId", &track.video_id),
            Op::Play { tracks, start, .. } => {
                if tracks.is_empty() || tracks.len() > MAX_TRACKS {
                    return Err(format!("tracks must have 1–{MAX_TRACKS} entries"));
                }
                if *start >= tracks.len() {
                    return Err("start is out of range".into());
                }
                tracks.iter().try_for_each(|t| short("videoId", &t.video_id))
            }
            Op::PlayTarget { target: WTarget::Browse { .. } } => Err("only watch targets can be played".into()),
            Op::PlayTarget { target: WTarget::Watch { video_id: None, playlist_id: None, .. } } => {
                Err("target needs a videoId or playlistId".into())
            }
            Op::Radio { video_id } => short("videoId", video_id),
            Op::Search { query } | Op::Suggest { query } => short("query", query),
            Op::Page { id, .. } => short("id", id),
            Op::More { token } => {
                if token.is_empty() || token.len() > 4096 {
                    Err("bad token".into())
                } else {
                    Ok(())
                }
            }
            Op::Rate { video_id, .. } => short("videoId", video_id),
            Op::AddToPlaylist { playlist_id, video_ids } => {
                short("playlistId", playlist_id)?;
                if video_ids.is_empty() || video_ids.len() > MAX_TRACKS {
                    return Err(format!("videoIds must have 1–{MAX_TRACKS} entries"));
                }
                video_ids.iter().try_for_each(|v| short("videoId", v))
            }
            Op::CreatePlaylist { title, video_ids } => {
                short("title", title)?;
                if video_ids.len() > MAX_TRACKS {
                    return Err("too many videoIds".into());
                }
                video_ids.iter().try_for_each(|v| short("videoId", v))
            }
            _ => Ok(()),
        }
    }
}

/// Parses one request line (protocol version, op, limits).
pub fn parse(line: &str) -> Result<Op, (&'static str, String)> {
    let v: Value = serde_json::from_str(line).map_err(|e| ("badRequest", format!("invalid JSON: {e}")))?;
    match v.get("v").and_then(Value::as_u64) {
        Some(n) if n == PROTOCOL as u64 => {}
        Some(n) => return Err(("unsupportedVersion", format!("protocol {n} isn't supported (expected {PROTOCOL})"))),
        None => return Err(("badRequest", "missing protocol version \"v\"".into())),
    }
    let op: Op = serde_json::from_value(v).map_err(|e| ("badRequest", e.to_string()))?;
    op.validate().map_err(|e| ("badRequest", e))?;
    Ok(op)
}

pub fn ok_line(result: Value) -> String {
    json!({ "v": PROTOCOL, "ok": true, "result": result }).to_string()
}

pub fn err_line(code: &str, message: &str) -> String {
    json!({ "v": PROTOCOL, "ok": false, "error": { "code": code, "message": message } }).to_string()
}

/// Where the app sends its answer; dropped unanswered = "failed".
pub struct Reply(mpsc::SyncSender<String>);

impl Reply {
    pub fn ok(self, result: Value) {
        let _ = self.0.send(ok_line(result));
    }

    pub fn err(self, code: &str, message: &str) {
        let _ = self.0.send(err_line(code, message));
    }

    pub fn result(self, r: Result<Value, String>) {
        match r {
            Ok(v) => self.ok(v),
            Err(e) => self.err("failed", &e),
        }
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

// =====================================================================
// server

pub fn socket_path() -> anyhow::Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| anyhow::anyhow!("XDG_RUNTIME_DIR isn't set"))?;
    Ok(dir.join("velour").join("control.sock"))
}

/// Creates the socket directory (0700); refuses one that is a symlink or belongs to someone else.
fn prepare_dir(dir: &Path) -> anyhow::Result<()> {
    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(e.into());
    }
    let m = std::fs::symlink_metadata(dir)?;
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    anyhow::ensure!(m.is_dir() && !m.file_type().is_symlink(), "{} isn't a directory", dir.display());
    anyhow::ensure!(m.uid() == uid, "{} belongs to another user", dir.display());
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub struct Server {
    stop: Arc<AtomicBool>,
    path: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listens on `path`; `handle` gets each valid request and must answer through the `Reply`
/// (from any thread). Fails if another live process already listens there.
pub fn start(path: PathBuf, handle: impl Fn(Op, Reply) + Send + Sync + 'static) -> anyhow::Result<Server> {
    prepare_dir(path.parent().ok_or_else(|| anyhow::anyhow!("bad socket path"))?)?;
    if std::fs::symlink_metadata(&path).is_ok() {
        anyhow::ensure!(UnixStream::connect(&path).is_err(), "another Velour is already listening");
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let handle = Arc::new(handle);
    {
        let stop = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new().name("velour-remote".into()).spawn(move || {
            // Polling accept keeps shutdown simple (no self-connect to wake it).
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                            let _ = writeln!(stream, "{}", err_line("busy", "too many requests at once"));
                            continue;
                        }
                        active.fetch_add(1, Ordering::SeqCst);
                        let (handle, done) = (handle.clone(), active.clone());
                        let spawned = std::thread::Builder::new().name("velour-remote-conn".into()).spawn(move || {
                            serve(stream, &*handle);
                            done.fetch_sub(1, Ordering::SeqCst);
                        });
                        if spawned.is_err() {
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100))
                    }
                    Err(e) => {
                        eprintln!("velour: remote control: {e}");
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            }
        })?;
    }
    Ok(Server { stop, path })
}

fn serve(stream: UnixStream, handle: &(impl Fn(Op, Reply) + ?Sized)) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(mut out) = stream.try_clone() else { return };
    let mut line = String::new();
    let read = BufReader::new(stream.take(MAX_REQUEST + 1)).read_line(&mut line);
    let answer = match read {
        Err(e) => err_line("badRequest", &format!("couldn’t read the request: {e}")),
        Ok(n) if n as u64 > MAX_REQUEST => err_line("tooLarge", "request is too large"),
        Ok(_) => match parse(line.trim_end()) {
            Err((code, msg)) => err_line(code, &msg),
            Ok(op) => {
                let (tx, rx) = mpsc::sync_channel(1);
                handle(op, Reply(tx));
                match rx.recv_timeout(REPLY_TIMEOUT) {
                    Ok(s) => s,
                    Err(mpsc::RecvTimeoutError::Timeout) => err_line("timeout", "Velour didn’t answer in time"),
                    Err(mpsc::RecvTimeoutError::Disconnected) => err_line("failed", "request was dropped"),
                }
            }
        },
    };
    let _ = writeln!(out, "{answer}");
}

// =====================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn track(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: "Song".into(),
            artists: vec![Link { name: "Artist".into(), browse_id: Some("UC1".into()) }],
            album: Some(Link { name: "Album".into(), browse_id: Some("MPRE1".into()) }),
            duration: Some(200),
            thumbnail: Some("https://lh3.googleusercontent.com/x".into()),
            set_video_id: Some("S1".into()),
            playable: true,
            like: Some(LikeStatus::Like),
            detail: None,
        }
    }

    #[test]
    fn track_round_trips_in_camel_case() {
        let t = track("abc");
        let w = WTrack::from(&t);
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["videoId"], "abc");
        assert_eq!(v["setVideoId"], "S1");
        assert_eq!(v["like"], "like");
        assert_eq!(v["artists"][0]["browseId"], "UC1");
        let back: WTrack = serde_json::from_value(v).unwrap();
        assert_eq!(Track::from(back), t);
    }

    #[test]
    fn minimal_track_gets_defaults() {
        let w: WTrack = serde_json::from_str(r#"{"videoId":"x","title":"T"}"#).unwrap();
        assert!(w.playable);
        assert!(w.artists.is_empty() && w.like.is_none());
    }

    #[test]
    fn page_is_tagged_by_kind() {
        let page = Page {
            header: Some(Header {
                title: "Mix".into(),
                play: Some(Target::Watch { video_id: None, playlist_id: Some("PL1".into()), params: None }),
                ..Default::default()
            }),
            sections: vec![Section {
                title: "Songs".into(),
                layout: Layout::List,
                items: vec![
                    Item::Track(track("a")),
                    Item::Card(Card {
                        title: "Album".into(),
                        subtitle: "2024".into(),
                        thumbnail: None,
                        target: Target::Browse { id: "MPRE1".into(), params: None, page_type: PageType::Album },
                        round: false,
                        wide: false,
                    }),
                ],
                more: None,
            }],
            continuation: Some("tok".into()),
        };
        let v = serde_json::to_value(wpage(&page)).unwrap();
        assert_eq!(v["header"]["play"], json!({"kind":"watch","videoId":null,"playlistId":"PL1","params":null}));
        assert_eq!(v["sections"][0]["layout"], "list");
        assert_eq!(v["sections"][0]["items"][0]["kind"], "track");
        assert_eq!(v["sections"][0]["items"][0]["videoId"], "a");
        assert_eq!(v["sections"][0]["items"][1]["kind"], "card");
        assert_eq!(v["sections"][0]["items"][1]["target"]["pageType"], "album");
        assert_eq!(v["continuation"], "tok");
    }

    #[test]
    fn target_round_trips() {
        for t in [
            Target::Browse { id: "VLPL1".into(), params: Some("p".into()), page_type: PageType::Playlist },
            Target::Watch { video_id: Some("v".into()), playlist_id: Some("RDAMVMv".into()), params: None },
        ] {
            let w: WTarget = serde_json::from_value(serde_json::to_value(WTarget::from(&t)).unwrap()).unwrap();
            assert_eq!(Target::from(w), t);
        }
    }

    #[test]
    fn parses_ops() {
        assert_eq!(parse(r#"{"v":1,"op":"state"}"#), Ok(Op::State));
        assert_eq!(parse(r#"{"v":1,"op":"seek","seconds":12.5}"#), Ok(Op::Seek { seconds: 12.5 }));
        assert_eq!(parse(r#"{"v":1,"op":"repeat","mode":"one"}"#), Ok(Op::Repeat { mode: WRepeat::One }));
        assert_eq!(parse(r#"{"v":1,"op":"library","tab":"albums"}"#), Ok(Op::Library { tab: LibraryTab::Albums }));
        assert_eq!(parse(r#"{"v":1,"op":"radio","videoId":"x"}"#), Ok(Op::Radio { video_id: "x".into() }));
        assert_eq!(
            parse(r#"{"v":1,"op":"add-to-playlist","playlistId":"PL","videoIds":["a","b"]}"#),
            Ok(Op::AddToPlaylist { playlist_id: "PL".into(), video_ids: vec!["a".into(), "b".into()] })
        );
        assert_eq!(parse(r#"{"v":1,"op":"page","id":"MPRE1"}"#), Ok(Op::Page { id: "MPRE1".into(), params: None }));
        let Ok(Op::Play { tracks, start: 1, shuffle: true }) = parse(
            r#"{"v":1,"op":"play","start":1,"shuffle":true,"tracks":[{"videoId":"a","title":"A"},{"videoId":"b","title":"B"}]}"#,
        ) else {
            panic!("play didn't parse")
        };
        assert_eq!(tracks.len(), 2);
    }

    #[test]
    fn rejects_bad_requests() {
        let code = |s: &str| parse(s).unwrap_err().0;
        assert_eq!(code("not json"), "badRequest");
        assert_eq!(code(r#"{"op":"state"}"#), "badRequest");
        assert_eq!(code(r#"{"v":2,"op":"state"}"#), "unsupportedVersion");
        assert_eq!(code(r#"{"v":1,"op":"format-disk"}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"seek"}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"seek","seconds":-1}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"volume","percent":101}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"search","query":"  "}"#), "badRequest");
        assert_eq!(code(&format!(r#"{{"v":1,"op":"search","query":"{}"}}"#, "a".repeat(301))), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"play","tracks":[]}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"play","start":1,"tracks":[{"videoId":"a","title":"A"}]}"#), "badRequest");
        assert_eq!(
            code(r#"{"v":1,"op":"play-target","target":{"kind":"browse","id":"MPRE1","pageType":"album"}}"#),
            "badRequest"
        );
        assert_eq!(code(r#"{"v":1,"op":"play-target","target":{"kind":"watch"}}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"add-to-playlist","playlistId":"PL","videoIds":[]}"#), "badRequest");
        assert_eq!(code(r#"{"v":1,"op":"rate","videoId":"x","like":"love"}"#), "badRequest");
    }

    #[test]
    fn lyrics_shape() {
        let r = LyricsResult { lyrics: Lyrics::Synced(vec![(1.5, "Hi".into())]), source: Some("LRCLIB".into()) };
        let v = wlyrics("x", &r);
        assert_eq!(v["kind"], "synced");
        assert_eq!(v["lines"][0], json!({"time":1.5,"text":"Hi"}));
        let v = wlyrics("x", &LyricsResult { lyrics: Lyrics::Plain("la".into()), source: None });
        assert_eq!((v["kind"].as_str(), v["text"].as_str()), (Some("plain"), Some("la")));
        assert_eq!(wlyrics("x", &LyricsResult { lyrics: Lyrics::None, source: None })["kind"], "none");
    }

    fn temp_socket() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "velour-remote-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        dir.join("velour").join("control.sock")
    }

    fn ask(path: &Path, body: &str) -> Value {
        let mut s = UnixStream::connect(path).unwrap();
        writeln!(s, "{body}").unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn server_answers_and_cleans_up() {
        let path = temp_socket();
        std::fs::create_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
        let server = start(path.clone(), |op, reply| match op {
            Op::State => reply.ok(json!({"playing": true})),
            Op::Next => reply.err("notFound", "nothing next"),
            _ => drop(reply),
        })
        .unwrap();

        let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        let sock_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!((dir_mode, sock_mode), (0o700, 0o600));

        assert_eq!(ask(&path, r#"{"v":1,"op":"state"}"#), json!({"v":1,"ok":true,"result":{"playing":true}}));
        let e = ask(&path, r#"{"v":1,"op":"next"}"#);
        assert_eq!((e["ok"].as_bool(), e["error"]["code"].as_str()), (Some(false), Some("notFound")));
        assert_eq!(ask(&path, r#"{"v":1,"op":"prev"}"#)["error"]["code"], "failed");
        assert_eq!(ask(&path, "garbage")["error"]["code"], "badRequest");

        // A second server can't take over a live socket.
        assert!(start(path.clone(), |_, r| drop(r)).is_err());
        drop(server);
        assert!(!path.exists());
        // A stale socket file (no listener) is replaced.
        let _stale = UnixListener::bind(&path).unwrap();
        drop(_stale);
        let again = start(path.clone(), |_, r| r.ok(json!(1))).unwrap();
        assert_eq!(ask(&path, r#"{"v":1,"op":"state"}"#)["result"], 1);
        drop(again);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn oversized_request_is_refused() {
        let path = temp_socket();
        std::fs::create_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
        let _server = start(path.clone(), |_, r| r.ok(json!(null))).unwrap();
        let mut s = UnixStream::connect(&path).unwrap();
        let big = vec![b'a'; (MAX_REQUEST + 10) as usize];
        let _ = s.write_all(&big);
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["code"], "tooLarge");
        drop(_server);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
