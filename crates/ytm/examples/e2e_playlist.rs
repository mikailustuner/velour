//! End-to-end check of playlist editing on a throwaway private playlist (deleted at the end).
//! Run: cargo run -p sonora-ytm --example e2e_playlist
//! Touches only the playlist it creates.

use anyhow::{Context, ensure};
use sonora_ytm::*;
use std::time::Duration;

const A: &str = "TtSr-J-SYMw";
const B: &str = "bnZoviZwjSc";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let c = Client::new(Cookies::from_firefox()?).await?;
    let id = c.create_playlist("Sonora test — safe to delete", &[A]).await?;
    println!("created {id}");
    let result = run(&c, &id).await;
    // Always clean up, even if a step failed.
    let deleted = c.delete_playlist(&id).await;
    println!("delete: {deleted:?}");
    result?;
    deleted?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let lib = c.library_playlists().await?;
    let still_there = lib
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .any(|i| matches!(i, Item::Card(Card { target: Target::Browse { id: b, .. }, .. }) if b.ends_with(&id)));
    ensure!(!still_there, "playlist still in library after delete");
    println!("OK: create, add, remove, rename, delete");
    Ok(())
}

async fn tracks(c: &Client, id: &str) -> anyhow::Result<(Page, Vec<Track>)> {
    // Edits take a moment to show up.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let p = c.page(&format!("VL{id}"), None).await?;
    let t = p.tracks();
    Ok((p, t))
}

async fn run(c: &Client, id: &str) -> anyhow::Result<()> {
    let (p, t) = tracks(c, id).await?;
    ensure!(p.header.as_ref().and_then(|h| h.playlist_id.as_deref()) == Some(id), "own playlist should be editable");
    ensure!(t.iter().map(|x| x.video_id.as_str()).collect::<Vec<_>>() == [A], "initial track: {t:?}");
    println!("create with song: ok");

    c.add_to_playlist(id, &[B]).await?;
    let (_, t) = tracks(c, id).await?;
    // YouTube Music may store the song's audio version under a different video id.
    ensure!(t.len() == 2, "added track missing: {t:?}");
    println!("add: ok");

    let first = t.iter().find(|x| x.video_id == A).context("first track")?;
    c.remove_from_playlist(id, A, first.set_video_id.as_deref().context("setVideoId")?).await?;
    let (_, t) = tracks(c, id).await?;
    ensure!(!t.iter().any(|x| x.video_id == A), "removed track still there");
    println!("remove: ok");

    c.rename_playlist(id, "Sonora test — renamed").await?;
    let (p, _) = tracks(c, id).await?;
    ensure!(p.header.as_ref().is_some_and(|h| h.title == "Sonora test — renamed"), "rename not visible");
    println!("rename: ok");
    Ok(())
}
