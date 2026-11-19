//! Cover art loading: sized URLs, disk cache, off-thread decode + downscale, small in-memory LRU.

use crate::settings::cache_dir;
use gtk::prelude::*;
use gtk::{gdk, glib};
use sha1::{Digest, Sha1};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use tokio::sync::Semaphore;

const MEMORY_ENTRIES: usize = 300;

thread_local! {
    static CACHE: RefCell<(HashMap<String, gdk::Texture>, VecDeque<String>)> = RefCell::new(Default::default());
}

fn limiter() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(6))
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap())
}

/// URL variant close to `px` pixels so we never download or decode more than needed.
pub fn url_for_size(url: &str, px: u32) -> String {
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        return sonora_ytm::parse::sized_thumbnail(url, px);
    }
    if let Some(rest) = url.split("i.ytimg.com/vi/").nth(1) {
        if let Some(id) = rest.split('/').next() {
            let name = if px <= 320 { "mqdefault.jpg" } else { "hqdefault.jpg" };
            return format!("https://i.ytimg.com/vi/{id}/{name}");
        }
    }
    url.to_string()
}

/// Widgets that can show a texture.
pub trait Target: IsA<gtk::Widget> {
    fn show(&self, tex: Option<&gdk::Texture>);
}

impl Target for gtk::Picture {
    fn show(&self, tex: Option<&gdk::Texture>) {
        self.set_paintable(tex);
    }
}

/// Fixed-size image (small covers): never grows with its parent.
impl Target for gtk::Image {
    fn show(&self, tex: Option<&gdk::Texture>) {
        match tex {
            Some(t) => self.set_paintable(Some(t)),
            None => self.clear(),
        }
    }
}

/// Shows `url` as a `px`-sized square (center-cropped).
pub fn load(picture: &impl Target, url: Option<&str>, px: u32) {
    load_with(picture, url, px, true);
}

/// Like `load` but keeps the aspect ratio (16:9 video tiles); `px` is the height.
pub fn load_wide(picture: &impl Target, url: Option<&str>, px: u32) {
    load_with(picture, url, px, false);
}

/// Like `load`, then calls `done` with the texture once it is shown.
pub fn load_then(picture: &impl Target, url: Option<&str>, px: u32, done: impl FnOnce(&gdk::Texture) + 'static) {
    load_inner(picture, url, px, true, Some(Box::new(done)));
}

fn load_with<W: Target>(picture: &W, url: Option<&str>, px: u32, square: bool) {
    load_inner(picture, url, px, square, None);
}

type Done = Box<dyn FnOnce(&gdk::Texture)>;

fn load_inner<W: Target>(picture: &W, url: Option<&str>, px: u32, square: bool, done: Option<Done>) {
    let Some(url) = url else {
        picture.show(None);
        return;
    };
    let url = url_for_size(url, px);
    let key = format!("{px}:{square}:{url}");
    // SAFETY: only `String` is ever stored under this key.
    unsafe { picture.set_data("sonora-image", key.clone()) };

    if let Some(tex) = CACHE.with(|c| c.borrow().0.get(&key).cloned()) {
        picture.show(Some(&tex));
        if let Some(f) = done {
            f(&tex);
        }
        return;
    }
    picture.show(None);

    let weak = picture.downgrade();
    let job = relm4::spawn(async move { fetch_decode(&url, px, square).await });
    glib::spawn_future_local(async move {
        let Ok(Some((w, h, rgba))) = job.await else { return };
        let bytes = glib::Bytes::from_owned(rgba);
        let tex: gdk::Texture = gdk::MemoryTexture::new(w as i32, h as i32, gdk::MemoryFormat::R8g8b8a8, &bytes, w as usize * 4).upcast();
        remember(&key, &tex);
        if let Some(p) = weak.upgrade() {
            // The widget may have been reused for another image meanwhile.
            let current = unsafe { p.data::<String>("sonora-image").map(|d| d.as_ref().clone()) };
            if current.as_deref() == Some(key.as_str()) {
                p.show(Some(&tex));
                if let Some(f) = done {
                    f(&tex);
                }
            }
        }
    });
}

fn remember(key: &str, tex: &gdk::Texture) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.0.insert(key.to_string(), tex.clone()).is_none() {
            c.1.push_back(key.to_string());
        }
        while c.1.len() > MEMORY_ENTRIES {
            if let Some(old) = c.1.pop_front() {
                c.0.remove(&old);
            }
        }
    });
}

async fn fetch_decode(url: &str, px: u32, square: bool) -> Option<(u32, u32, Vec<u8>)> {
    let path = cache_dir().join("images").join(hex::encode(Sha1::digest(url.as_bytes())));
    let data = match tokio::fs::read(&path).await {
        Ok(d) => d,
        Err(_) => {
            let _permit = limiter().acquire().await.ok()?;
            let bytes = http().get(url).send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?.to_vec();
            if let Some(dir) = path.parent() {
                let _ = tokio::fs::create_dir_all(dir).await;
            }
            let _ = tokio::fs::write(&path, &bytes).await;
            bytes
        }
    };
    tokio::task::spawn_blocking(move || {
        let mut img = image::load_from_memory(&data).ok()?;
        if square && img.width() != img.height() {
            let side = img.width().min(img.height());
            img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
        }
        // Scale so the height matches `px`; width follows the aspect ratio.
        let img = if img.height() > px {
            let w = (img.width() as f64 * px as f64 / img.height() as f64).round().max(1.0) as u32;
            img.thumbnail(w, px)
        } else {
            img
        };
        let rgba = img.to_rgba8();
        Some((rgba.width(), rgba.height(), rgba.into_raw()))
    })
    .await
    .ok()
    .flatten()
}

/// A vivid representative color of a (small) cover, for the ambient glow.
/// Saturated pixels count more than grey ones; the result is lifted so it shows on black.
pub fn accent_color(tex: &gdk::Texture) -> Option<(u8, u8, u8)> {
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    if w == 0 || h == 0 || w * h > 512 * 512 {
        return None;
    }
    let mut buf = vec![0u8; w * h * 4];
    tex.download(&mut buf, w * 4); // B8G8R8A8 premultiplied
    let (mut r, mut g, mut b, mut total) = (0f64, 0f64, 0f64, 0f64);
    for px in buf.chunks_exact(4) {
        let (pb, pg, pr) = (px[0] as f64, px[1] as f64, px[2] as f64);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        // Ignore near-black/near-white; favor colorful pixels.
        if max < 30.0 || min > 235.0 {
            continue;
        }
        let weight = 0.05 + sat * sat;
        r += pr * weight;
        g += pg * weight;
        b += pb * weight;
        total += weight;
    }
    if total == 0.0 {
        return None;
    }
    let (r, g, b) = (r / total, g / total, b / total);
    let max = r.max(g).max(b).max(1.0);
    let lift = 220.0 / max;
    Some(((r * lift).min(255.0) as u8, (g * lift).min(255.0) as u8, (b * lift).min(255.0) as u8))
}

/// Deletes cached image files older than 30 days.
pub fn prune_disk_cache() {
    std::thread::spawn(|| {
        let dir = cache_dir().join("images");
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 3600);
        for e in entries.flatten() {
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_small_variants() {
        assert_eq!(
            url_for_size("https://lh3.googleusercontent.com/x=w60-h60-l90-rj", 160),
            "https://lh3.googleusercontent.com/x=w160-h160-l90-rj"
        );
        assert_eq!(url_for_size("https://i.ytimg.com/vi/abc/hq720.jpg?sqp=1", 160), "https://i.ytimg.com/vi/abc/mqdefault.jpg");
    }
}
