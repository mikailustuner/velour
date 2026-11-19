//! High-level YouTube Music operations used by the app.

use crate::client::Client;
use crate::model::*;
use crate::parse;
use anyhow::{Context, Result, bail};
use rand::Rng;
use serde_json::{Value, json};

impl Client {
    pub async fn home(&self) -> Result<Page> {
        self.page("FEmusic_home", None).await
    }

    pub async fn explore(&self) -> Result<Page> {
        self.page("FEmusic_explore", None).await
    }

    pub async fn liked_songs(&self) -> Result<Page> {
        self.page("VLLM", None).await
    }

    pub async fn history(&self) -> Result<Page> {
        self.page("FEmusic_history", None).await
    }

    pub async fn library_playlists(&self) -> Result<Page> {
        self.page("FEmusic_liked_playlists", None).await
    }

    pub async fn library_albums(&self) -> Result<Page> {
        self.page("FEmusic_liked_albums", None).await
    }

    pub async fn library_artists(&self) -> Result<Page> {
        self.page("FEmusic_library_corpus_track_artists", None).await
    }

    /// Any browse page (playlist `VL…`, album `MPRE…`, artist `UC…`, mood category…).
    pub async fn page(&self, browse_id: &str, params: Option<&str>) -> Result<Page> {
        let mut body = json!({ "browseId": browse_id });
        if let Some(p) = params {
            body["params"] = json!(p);
        }
        let v = self.call("browse", body).await?;
        Ok(parse::page(&v, browse_id))
    }

    /// Next chunk of a playlist / section list.
    pub async fn more(&self, token: &str) -> Result<Page> {
        let q = format!("browse?ctoken={token}&continuation={token}&type=next");
        let v = self.call(&q, json!({})).await?;
        let page = parse::continuation(&v);
        if !page.sections.is_empty() {
            return Ok(page);
        }
        // Newer pages take the token in the body.
        let v = self.call("browse", json!({ "continuation": token })).await?;
        Ok(parse::continuation(&v))
    }

    /// Display name of the signed-in account (to tell own playlists apart).
    pub async fn account_name(&self) -> Result<String> {
        let v = self.call("account/account_menu", json!({})).await?;
        parse::first(&v, "accountName").map(parse::text).filter(|s| !s.is_empty()).context("account name not found")
    }

    pub async fn search(&self, query: &str) -> Result<Page> {
        let v = self.call("search", json!({ "query": query })).await?;
        Ok(parse::search(&v))
    }

    pub async fn search_suggestions(&self, input: &str) -> Result<Vec<String>> {
        let v = self.call("music/get_search_suggestions", json!({ "input": input })).await?;
        let mut out = vec![];
        parse::find_all(&v, "searchSuggestionRenderer", &mut out);
        Ok(out.iter().map(|s| parse::text(&s["suggestion"])).filter(|s| !s.is_empty()).collect())
    }

    /// Queue for playing `video_id` and/or `playlist_id` (radio when the id starts with `RD`).
    pub async fn queue(
        &self,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
        params: Option<&str>,
    ) -> Result<Queue> {
        let mut body = json!({ "isAudioOnly": true, "enablePersistentPlaylistPanel": true, "tunerSettingValue": "AUTOMIX_SETTING_NORMAL" });
        if let Some(v) = video_id {
            body["videoId"] = json!(v);
        }
        if let Some(p) = playlist_id {
            body["playlistId"] = json!(p);
        }
        if let Some(p) = params {
            body["params"] = json!(p);
        }
        if video_id.is_none() && playlist_id.is_none() {
            bail!("queue needs a video or playlist id");
        }
        let v = self.call("next", body).await?;
        Ok(parse::queue(&v))
    }

    /// Endless radio: more tracks for a queue that has a continuation.
    pub async fn queue_more(&self, token: &str, playlist_id: &str) -> Result<Queue> {
        let q = format!("next?ctoken={token}&continuation={token}&type=next");
        let v = self
            .call(&q, json!({ "playlistId": playlist_id, "isAudioOnly": true, "enablePersistentPlaylistPanel": true }))
            .await?;
        Ok(parse::queue(&v))
    }

    /// Radio for a song.
    pub async fn radio(&self, video_id: &str) -> Result<Queue> {
        self.queue(Some(video_id), Some(&format!("RDAMVM{video_id}")), Some("wAEB")).await
    }

    /// Plain lyrics from YouTube Music (synced lyrics come from LRCLIB in the app).
    pub async fn lyrics(&self, video_id: &str) -> Result<Option<Lyrics>> {
        let q = self.queue(Some(video_id), None, None).await?;
        let Some(id) = q.lyrics_browse_id else { return Ok(None) };
        let v = self.call("browse", json!({ "browseId": id })).await?;
        Ok(parse::lyrics(&v))
    }

    pub async fn rate_song(&self, video_id: &str, status: LikeStatus) -> Result<()> {
        let endpoint = match status {
            LikeStatus::Like => "like/like",
            LikeStatus::Dislike => "like/dislike",
            LikeStatus::Indifferent => "like/removelike",
        };
        self.call(endpoint, json!({ "target": { "videoId": video_id } })).await?;
        Ok(())
    }

    /// Save/remove an album or someone else's playlist to/from the library.
    pub async fn rate_playlist(&self, playlist_id: &str, save: bool) -> Result<()> {
        let endpoint = if save { "like/like" } else { "like/removelike" };
        self.call(endpoint, json!({ "target": { "playlistId": playlist_id } })).await?;
        Ok(())
    }

    pub async fn add_to_playlist(&self, playlist_id: &str, video_ids: &[&str]) -> Result<()> {
        let actions: Vec<Value> = video_ids
            .iter()
            .map(|v| json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": v, "dedupeOption": "DEDUPE_OPTION_SKIP" }))
            .collect();
        let v = self
            .call("browse/edit_playlist", json!({ "playlistId": strip_vl(playlist_id), "actions": actions }))
            .await?;
        check_status(&v)
    }

    pub async fn remove_from_playlist(&self, playlist_id: &str, video_id: &str, set_video_id: &str) -> Result<()> {
        let actions =
            json!([{ "action": "ACTION_REMOVE_VIDEO", "removedVideoId": video_id, "setVideoId": set_video_id }]);
        let v = self
            .call("browse/edit_playlist", json!({ "playlistId": strip_vl(playlist_id), "actions": actions }))
            .await?;
        check_status(&v)
    }

    pub async fn rename_playlist(&self, playlist_id: &str, title: &str) -> Result<()> {
        let actions = json!([{ "action": "ACTION_SET_PLAYLIST_NAME", "playlistName": title }]);
        let v = self
            .call("browse/edit_playlist", json!({ "playlistId": strip_vl(playlist_id), "actions": actions }))
            .await?;
        check_status(&v)
    }

    /// Creates a private playlist and returns its id.
    pub async fn create_playlist(&self, title: &str, video_ids: &[&str]) -> Result<String> {
        let v = self
            .call(
                "playlist/create",
                json!({ "title": title, "description": "", "privacyStatus": "PRIVATE", "videoIds": video_ids }),
            )
            .await?;
        v["playlistId"].as_str().map(str::to_string).context("playlist/create returned no id")
    }

    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.call("playlist/delete", json!({ "playlistId": strip_vl(playlist_id) })).await?;
        Ok(())
    }

    /// Records a play in the account's YouTube Music history (feeds recommendations).
    pub async fn add_history(&self, video_id: &str) -> Result<()> {
        let v = self.call("player", json!({ "videoId": video_id })).await?;
        let url =
            v["playbackTracking"]["videostatsPlaybackUrl"]["baseUrl"].as_str().context("no playback tracking url")?;
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        // Keep the thread-local RNG out of the await points (the future must be Send).
        let cpn: String = {
            let mut rng = rand::rng();
            (0..16).map(|_| CHARS[rng.random_range(0..CHARS.len())] as char).collect()
        };
        self.http()
            .get(format!("{url}&ver=2&c=WEB_REMIX&cpn={cpn}"))
            .header("Cookie", self.cookie_header())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

fn strip_vl(id: &str) -> &str {
    id.strip_prefix("VL").unwrap_or(id)
}

fn check_status(v: &Value) -> Result<()> {
    match v["status"].as_str() {
        Some("STATUS_SUCCEEDED") | None => Ok(()),
        Some(s) => bail!("playlist edit failed: {s}"),
    }
}
