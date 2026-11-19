//! Dev check: resolve two tracks and play them back-to-back gaplessly (ao=null by default).
//! Run: cargo run -p sonora-player --example play -- [ao]

use sonora_player::{Event, Player, Resolver};
use std::sync::mpsc;

fn main() -> anyhow::Result<()> {
    let ao = std::env::args().nth(1).unwrap_or_else(|| "null".into());
    let profile = sonora_ytm::auth::default_firefox_profile()?;
    let resolver = Resolver::new(profile);

    let t = std::time::Instant::now();
    let a = resolver.resolve("TtSr-J-SYMw")?;
    println!("resolved 1 in {:?} (itag {})", t.elapsed(), itag(&a));
    let t = std::time::Instant::now();
    let b = resolver.resolve("bnZoviZwjSc")?;
    println!("resolved 2 in {:?} (itag {})", t.elapsed(), itag(&b));
    let t = std::time::Instant::now();
    resolver.resolve("TtSr-J-SYMw")?;
    println!("cached lookup in {:?}", t.elapsed());

    let (tx, rx) = mpsc::channel();
    let player = Player::new(Some(&ao), move |e| {
        let _ = tx.send(e);
    })?;
    player.play(&a, "TtSr-J-SYMw")?;
    player.set_next(&b, "bnZoviZwjSc")?;

    let mut seeked = false;
    while let Ok(e) = rx.recv() {
        match &e {
            Event::Started { id } => println!("started {id}"),
            Event::Duration(d) if !seeked => {
                // Jump near the end to see the gapless switch quickly.
                player.seek(d - 3.0)?;
                seeked = true;
                println!("duration {d:.0}s, seeking to end");
            }
            Event::Ended { eof } => println!("ended eof={eof}"),
            Event::Error(err) => println!("error {err}"),
            Event::Position(p) if seeked && *p > 2.0 && *p < 10.0 => {
                println!("second track playing at {p:.1}s");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

fn itag(url: &str) -> &str {
    url.split("itag=").nth(1).and_then(|s| s.split('&').next()).unwrap_or("?")
}
