//! Parser tests against real responses saved by `cargo run -p velour-ytm --example dump`.
//!
//! `fixtures/public` is fetched signed out and committed, so those tests always run.
//! `fixtures/private` comes from your own account and is gitignored; those tests skip
//! when the files are missing. Assertions check structure, not specific songs, so
//! re-dumping on another day or account keeps them valid.

use serde_json::Value;
use velour_ytm::*;

fn load(kind: &str, name: &str) -> Option<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{kind}/{name}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).unwrap())
}

fn public(name: &str) -> Value {
    load("public", name).unwrap_or_else(|| panic!("missing public fixture {name}; run the dump example"))
}

macro_rules! private_or_skip {
    ($name:expr) => {
        match load("private", $name) {
            Some(v) => v,
            None => {
                eprintln!("skipping: private fixture {} missing", $name);
                return;
            }
        }
    };
}

fn section<'a>(page: &'a Page, title: &str) -> &'a Section {
    page.sections.iter().find(|s| s.title == title).unwrap_or_else(|| {
        panic!("no section {title:?}; have {:?}", page.sections.iter().map(|s| &s.title).collect::<Vec<_>>())
    })
}

fn cards(page: &Page) -> Vec<&Card> {
    page.sections
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|i| match i {
            Item::Card(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn assert_track_complete(t: &Track) {
    assert_eq!(t.video_id.len(), 11, "{t:?}");
    assert!(!t.title.is_empty(), "{t:?}");
    assert!(!t.artists.is_empty(), "{t:?}");
}

/// "5 views • 79 tracks" → 79, "49 songs" → 49.
fn count_in(text: &str) -> Option<usize> {
    text.split(" • ")
        .filter(|p| p.ends_with("tracks") || p.ends_with("songs") || p.ends_with("track") || p.ends_with("song"))
        .find_map(|p| p.split_whitespace().next()?.replace(',', "").parse().ok())
}

// ---------------- public ----------------

#[test]
fn home_has_titled_shelves() {
    let p = parse::page(&public("home"), "FEmusic_home");
    assert!(p.sections.len() >= 2, "{} sections", p.sections.len());
    for s in &p.sections {
        assert!(!s.title.is_empty());
        assert!(!s.items.is_empty());
    }
}

#[test]
fn explore_has_moods_with_params() {
    let p = parse::page(&public("explore"), "FEmusic_explore");
    let moods = section(&p, "Moods & genres");
    assert!(
        moods
            .items
            .iter()
            .all(|i| matches!(i, Item::Card(Card { target: Target::Browse { params: Some(_), .. }, .. })))
    );
    assert!(p.sections.iter().any(|s| s.title.contains("New")), "a new releases shelf");
}

#[test]
fn artist_page() {
    let p = parse::page(&public("artist"), "UCQm-Fc8TAF3c1hJffBPjxgw");
    let h = p.header.as_ref().unwrap();
    assert_eq!(h.title, "Tarkan");
    assert!(h.round);
    assert!(h.thumbnail.is_some());
    assert!(h.radio.is_some() || h.play.is_some());
    let top = section(&p, "Top songs");
    assert_eq!(top.layout, Layout::List);
    assert!(!top.items.is_empty());
    let albums = section(&p, "Albums");
    assert!(
        albums
            .items
            .iter()
            .all(|i| matches!(i, Item::Card(Card { target: Target::Browse { page_type: PageType::Album, .. }, .. })))
    );
}

#[test]
fn album_page() {
    let p = parse::page(&public("album"), "MPREb_nWmOXlZBVTL");
    let h = p.header.as_ref().unwrap();
    assert!(!h.title.is_empty());
    assert!(
        h.subtitle.contains("Single") || h.subtitle.contains("Album") || h.subtitle.contains("EP"),
        "{}",
        h.subtitle
    );
    match &h.play {
        Some(Target::Watch { playlist_id: Some(id), .. }) => assert!(id.starts_with("OLAK"), "{id}"),
        other => panic!("album play target: {other:?}"),
    }
    let tracks = p.tracks();
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| t.duration.is_some()));
}

#[test]
fn public_playlist_is_not_editable() {
    let p = parse::page(&public("public_playlist"), "VLRDCLAK5uy_nd-EeKkaNaajGKQSdacTFGy2M9tnjlMK0");
    let h = p.header.as_ref().unwrap();
    assert_eq!(h.playlist_id, None, "someone else's playlist");
    let tracks = p.tracks();
    if let Some(n) = count_in(&h.second_subtitle) {
        assert!(tracks.len() == n || p.continuation.is_some(), "{} of {n} tracks without continuation", tracks.len());
    }
    tracks.iter().filter(|t| t.playable).for_each(assert_track_complete);
}

#[test]
fn search_groups_results() {
    let p = parse::search(&public("search"));
    assert_eq!(p.sections[0].title, "Top result");
    let songs = section(&p, "Songs");
    assert!(songs.items.len() >= 3);
    for i in &songs.items {
        let Item::Track(t) = i else { panic!("non-track in Songs") };
        assert_track_complete(t);
        assert!(t.detail.as_deref().is_none_or(|d| !d.starts_with("Episode")), "episodes are filtered");
    }
    for title in ["Albums", "Artists"] {
        assert!(!section(&p, title).items.is_empty());
    }
}

#[test]
fn queue_and_radio() {
    let q = parse::queue(&public("next"));
    assert!(!q.tracks.is_empty());
    let r = parse::queue(&public("radio"));
    assert!(r.tracks.len() >= 20, "{} radio tracks", r.tracks.len());
    assert!(r.continuation.is_some(), "radio continues");
    assert!(r.tracks.iter().all(|t| !t.title.is_empty() && t.duration.is_some()));
}

// ---------------- private (your account) ----------------

#[test]
fn liked_songs() {
    let v = private_or_skip!("liked");
    let p = parse::page(&v, "VLLM");
    let h = p.header.as_ref().unwrap();
    assert_eq!(h.title, "Liked Music");
    assert_eq!(h.play, Some(Target::Watch { video_id: None, playlist_id: Some("LM".into()), params: None }));
    for t in p.tracks().iter().filter(|t| t.playable) {
        assert_track_complete(t);
        assert_eq!(t.like, Some(LikeStatus::Like));
    }
}

#[test]
fn own_playlist_is_editable() {
    let v = private_or_skip!("own_playlist");
    let p = parse::page(&v, "VLown");
    let h = p.header.as_ref().unwrap();
    assert!(h.playlist_id.is_some(), "own playlists expose their id for editing");
    let tracks = p.tracks();
    assert!(tracks.iter().all(|t| t.set_video_id.is_some()), "needed for removing tracks");
}

#[test]
fn library_playlists() {
    let v = private_or_skip!("library_playlists");
    let p = parse::page(&v, "FEmusic_liked_playlists");
    assert!(cards(&p).iter().any(|c| c.title == "Liked Music"));
}

#[test]
fn history() {
    let v = private_or_skip!("history");
    let p = parse::page(&v, "FEmusic_history");
    p.tracks().iter().filter(|t| t.playable).for_each(assert_track_complete);
}

// ---------------- helpers ----------------

#[test]
fn durations_and_thumbs() {
    assert_eq!(parse::parse_duration("3:14"), Some(194));
    assert_eq!(parse::parse_duration("1:02:03"), Some(3723));
    assert_eq!(parse::parse_duration("Song"), None);
    assert_eq!(
        parse::sized_thumbnail("https://lh3.googleusercontent.com/abc=w60-h60-l90-rj", 240),
        "https://lh3.googleusercontent.com/abc=w240-h240-l90-rj"
    );
    assert_eq!(parse::sized_thumbnail("https://i.ytimg.com/vi/x/hq.jpg", 240), "https://i.ytimg.com/vi/x/hq.jpg");
    assert_eq!(count_in("49 songs • 2 hours"), Some(49));
    assert_eq!(count_in("Playlist • 1,234 tracks"), Some(1234));
    assert_eq!(count_in("5 views • 79 tracks • 4 hours"), Some(79));
}
