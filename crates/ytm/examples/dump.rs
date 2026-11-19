//! Dev tool: save InnerTube responses as parser test fixtures.
//!
//!   cargo run -p sonora-ytm --example dump -- public    # signed out → tests/fixtures/public (committed)
//!   cargo run -p sonora-ytm --example dump -- private   # your account → tests/fixtures/private (gitignored)
//!
//! Tracking/telemetry keys are stripped before saving.

use serde_json::{Value, json};
use sonora_ytm::{Card, Client, Cookies, Item, PageType, Target};
use std::path::{Path, PathBuf};

/// Public, stable ids used for committed fixtures.
const ARTIST: &str = "UCQm-Fc8TAF3c1hJffBPjxgw"; // Tarkan
const ALBUM: &str = "MPREb_nWmOXlZBVTL"; // "Wake Up" (single)
const PLAYLIST: &str = "VLRDCLAK5uy_nd-EeKkaNaajGKQSdacTFGy2M9tnjlMK0"; // YouTube Music editorial playlist
const SONG: &str = "TtSr-J-SYMw";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    match mode.as_str() {
        "public" => public(&base.join("public")).await,
        "private" => private(&base.join("private")).await,
        _ => anyhow::bail!("usage: dump public|private"),
    }
}

async fn public(dir: &Path) -> anyhow::Result<()> {
    let c = Client::anonymous().await?;
    for (name, id) in [
        ("home", "FEmusic_home"),
        ("explore", "FEmusic_explore"),
        ("artist", ARTIST),
        ("album", ALBUM),
        ("public_playlist", PLAYLIST),
    ] {
        save(dir, name, c.browse(id).await?);
    }
    save(dir, "search", c.call("search", json!({ "query": "tarkan" })).await?);
    save(dir, "next", c.call("next", json!({ "videoId": SONG, "isAudioOnly": true })).await?);
    let radio =
        json!({ "videoId": SONG, "playlistId": format!("RDAMVM{SONG}"), "params": "wAEB", "isAudioOnly": true });
    save(dir, "radio", c.call("next", radio).await?);
    Ok(())
}

async fn private(dir: &Path) -> anyhow::Result<()> {
    let c = Client::new(Cookies::from_firefox()?).await?;
    for (name, id) in [
        ("liked", "VLLM"),
        ("history", "FEmusic_history"),
        ("library_playlists", "FEmusic_liked_playlists"),
        ("library_albums", "FEmusic_liked_albums"),
        ("library_artists", "FEmusic_library_corpus_track_artists"),
    ] {
        save(dir, name, c.browse(id).await?);
    }
    // First playlist owned by the signed-in account (for the editable-playlist test).
    let account = c.account_name().await?;
    let lib = c.library_playlists().await?;
    let own = lib.sections.iter().flat_map(|s| &s.items).find_map(|i| match i {
        Item::Card(Card { subtitle, target: Target::Browse { id, page_type: PageType::Playlist, .. }, .. })
            if subtitle.contains(&account) && id != "VLLM" =>
        {
            Some(id.clone())
        }
        _ => None,
    });
    match own {
        Some(id) => save(dir, "own_playlist", c.browse(&id).await?),
        None => println!("no own playlist found; skipping own_playlist"),
    }
    Ok(())
}

fn save(dir: &Path, name: &str, mut v: Value) {
    strip(&mut v);
    std::fs::create_dir_all(dir).unwrap();
    let path: PathBuf = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
    println!("saved {}", path.display());
}

/// Drops telemetry and session identifiers the parsers never read.
fn strip(v: &mut Value) {
    const KEYS: [&str; 9] = [
        "responseContext",
        "trackingParams",
        "clickTrackingParams",
        "loggingContext",
        "loggingDirectives",
        "frameworkUpdates",
        "adSignalsInfo",
        "serializedShareEntity",
        "playerParams",
    ];
    match v {
        Value::Object(m) => {
            for k in KEYS {
                m.remove(k);
            }
            m.values_mut().for_each(strip);
        }
        Value::Array(a) => a.iter_mut().for_each(strip),
        _ => {}
    }
}
