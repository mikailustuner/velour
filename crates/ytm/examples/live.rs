//! Dev check of live endpoints that fixtures don't cover.
use velour_ytm::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let c = Client::new(Cookies::from_firefox()?).await?;
    println!("account: {:?}", c.account_name().await);
    let home = c.home().await?;
    println!("home: {} sections, cont={}", home.sections.len(), home.continuation.is_some());
    if let Some(t) = &home.continuation {
        let more = c.more(t).await?;
        println!(
            "home more: {} sections {:?}",
            more.sections.len(),
            more.sections.iter().map(|s| &s.title).collect::<Vec<_>>()
        );
    }
    let radio = c.radio("TtSr-J-SYMw").await?;
    println!(
        "radio: {} tracks, first={:?}",
        radio.tracks.len(),
        radio.tracks.first().map(|t| (&t.title, t.artist_line()))
    );
    if let (Some(tok), Some(pl)) = (&radio.continuation, &radio.playlist_id) {
        let more = c.queue_more(tok, pl).await?;
        println!("radio more: {} tracks, cont={}", more.tracks.len(), more.continuation.is_some());
    }
    let pl = c.queue(None, Some("RDCLAK5uy_nd-EeKkaNaajGKQSdacTFGy2M9tnjlMK0"), None).await?;
    println!("playlist queue: {} tracks", pl.tracks.len());
    match c.lyrics("TtSr-J-SYMw").await? {
        Some(l) => println!("lyrics: {} chars, source={:?}", l.text.len(), l.source),
        None => println!("lyrics: none"),
    }
    println!("suggest: {:?}", c.search_suggestions("tark").await?);
    // A playlist with >100 tracks to exercise continuation.
    let big = c.search(&std::env::args().nth(1).unwrap_or("top 100 turkey".into())).await?;
    let target = big.sections.iter().flat_map(|s| &s.items).find_map(|i| match i {
        Item::Card(Card { target: Target::Browse { id, page_type: PageType::Playlist, .. }, .. }) => Some(id.clone()),
        _ => None,
    });
    if let Some(id) = target {
        let p = c.page(&id, None).await?;
        print!("playlist {id}: {} tracks, cont={}", p.tracks().len(), p.continuation.is_some());
        if let Some(t) = &p.continuation {
            let m = c.more(t).await?;
            print!(" → more {} tracks", m.sections.iter().map(|s| s.items.len()).sum::<usize>());
        }
        println!();
    }
    let a = c.library_albums().await?;
    println!("library albums: {} cards", a.sections.iter().map(|s| s.items.len()).sum::<usize>());
    let ar = c.library_artists().await?;
    println!("library artists: {} items", ar.sections.iter().map(|s| s.items.len()).sum::<usize>());
    Ok(())
}
