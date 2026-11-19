//! Spike: log in with Firefox cookies and list library playlists.
//! Run: cargo run -p sonora-ytm --example library

use serde_json::Value;
use sonora_ytm::{Client, Cookies};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::new(Cookies::from_firefox()?).await?;
    println!("client version: {}", client.client_version());

    let resp = client.browse("FEmusic_liked_playlists").await?;
    let mut items = Vec::new();
    collect(&resp, "musicTwoRowItemRenderer", &mut items);
    println!("{} playlists:", items.len());
    for item in items {
        let title = runs(&item["title"]);
        let subtitle = runs(&item["subtitle"]);
        println!("  {title}  —  {subtitle}");
    }
    Ok(())
}

fn collect(v: &Value, key: &str, out: &mut Vec<Value>) {
    match v {
        Value::Object(m) => {
            for (k, child) in m {
                if k == key { out.push(child.clone()) } else { collect(child, key, out) }
            }
        }
        Value::Array(a) => a.iter().for_each(|c| collect(c, key, out)),
        _ => {}
    }
}

fn runs(v: &Value) -> String {
    v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default()
}
