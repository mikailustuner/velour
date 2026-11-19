//! Tolerant InnerTube JSON → model conversion. Missing fields degrade to defaults instead of
//! failing, because YouTube changes response shapes often and partial data beats an error page.

use crate::model::*;
use serde_json::Value;

// ---------- generic helpers ----------

/// Text of a `{runs:[...]}` or `{simpleText}` object.
pub fn text(v: &Value) -> String {
    if let Some(s) = v["simpleText"].as_str() {
        return s.to_string();
    }
    v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default()
}

/// All values stored under `key`, depth-first, without descending into matches.
pub fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                if k == key { out.push(c) } else { find_all(c, key, out) }
            }
        }
        Value::Array(a) => a.iter().for_each(|c| find_all(c, key, out)),
        _ => {}
    }
}

pub fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|c| first(c, key))),
        Value::Array(a) => a.iter().find_map(|c| first(c, key)),
        _ => None,
    }
}

fn str_of(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// Largest thumbnail URL found under `v`.
pub(crate) fn thumbnail(v: &Value) -> Option<String> {
    let thumbs = first(v, "thumbnails")?.as_array()?;
    thumbs
        .iter()
        .max_by_key(|t| t["width"].as_u64().unwrap_or(0))
        .and_then(|t| str_of(&t["url"]))
        .map(|u| if u.starts_with("//") { format!("https:{u}") } else { u })
}

/// Rewrites googleusercontent thumbnail URLs to the requested square size.
pub fn sized_thumbnail(url: &str, size: u32) -> String {
    if let Some(pos) = url.rfind("=w") {
        if url[pos..].contains("-h") {
            return format!("{}=w{size}-h{size}-l90-rj", &url[..pos]);
        }
    }
    url.to_string()
}

pub fn parse_duration(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || !s.contains(':') {
        return None;
    }
    s.split(':').try_fold(0u32, |acc, part| part.parse::<u32>().ok().map(|n| acc * 60 + n))
}

pub(crate) fn target(endpoint: &Value) -> Option<Target> {
    if let Some(b) = endpoint.get("browseEndpoint") {
        let id = str_of(&b["browseId"])?;
        let pt = b["browseEndpointContextSupportedConfigs"]["browseEndpointContextMusicConfig"]["pageType"].as_str();
        return Some(Target::Browse { page_type: PageType::from_api(pt, &id), params: str_of(&b["params"]), id });
    }
    if let Some(w) = endpoint.get("watchEndpoint") {
        return Some(Target::Watch {
            video_id: str_of(&w["videoId"]),
            playlist_id: str_of(&w["playlistId"]),
            params: str_of(&w["params"]),
        });
    }
    if let Some(w) = endpoint.get("watchPlaylistEndpoint") {
        return Some(Target::Watch { video_id: None, playlist_id: str_of(&w["playlistId"]), params: str_of(&w["params"]) });
    }
    None
}

fn is_separator(s: &str) -> bool {
    matches!(s.trim(), "" | "•" | "&" | "," | "·")
}

const KIND_LABELS: [&str; 9] = ["Song", "Video", "Episode", "Single", "EP", "Album", "Playlist", "Artist", "Podcast"];

fn looks_like_stat(s: &str) -> bool {
    let l = s.to_lowercase();
    ["views", "plays", "likes", "subscribers", "monthly", "audience", "songs", "tracks"]
        .iter()
        .any(|w| l.contains(w))
        || (s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()))
}

fn like_status(v: &Value) -> Option<LikeStatus> {
    match first(v, "likeStatus")?.as_str()? {
        "LIKE" => Some(LikeStatus::Like),
        "DISLIKE" => Some(LikeStatus::Dislike),
        _ => Some(LikeStatus::Indifferent),
    }
}

/// Splits byline runs into artists, album, duration and leftover detail text.
struct Byline {
    artists: Vec<Link>,
    album: Option<Link>,
    duration: Option<u32>,
    detail: Vec<String>,
    kind: Option<String>,
}

fn byline(runs: &[&Value]) -> Byline {
    let mut b = Byline { artists: vec![], album: None, duration: None, detail: vec![], kind: None };
    for (i, run) in runs.iter().enumerate() {
        let t = run["text"].as_str().unwrap_or("").trim();
        if is_separator(t) {
            continue;
        }
        if let Some(Target::Browse { id, page_type, .. }) = target(&run["navigationEndpoint"]) {
            let link = Link { name: t.to_string(), browse_id: Some(id) };
            match page_type {
                PageType::Album => b.album = Some(link),
                PageType::Artist | PageType::Channel => b.artists.push(link),
                _ => b.detail.push(t.to_string()),
            }
            continue;
        }
        if let Some(d) = parse_duration(t) {
            b.duration = Some(d);
        } else if i == 0 && KIND_LABELS.contains(&t) {
            b.kind = Some(t.to_string());
        } else if looks_like_stat(t) {
            b.detail.push(t.to_string());
        } else if b.album.is_none() && b.duration.is_none() && b.detail.is_empty() {
            // Unlinked artist names (e.g. "Song • Tarkan" in search).
            b.artists.push(Link { name: t.to_string(), browse_id: None });
        } else {
            b.detail.push(t.to_string());
        }
    }
    b
}

fn runs_of(v: &Value) -> Vec<&Value> {
    v["runs"].as_array().map(|a| a.iter().collect()).unwrap_or_default()
}

// ---------- items ----------

pub(crate) fn list_item(r: &Value) -> Option<Item> {
    let cols: Vec<&Value> = r["flexColumns"]
        .as_array()?
        .iter()
        .map(|c| &c["musicResponsiveListItemFlexColumnRenderer"]["text"])
        .collect();
    let title = text(cols.first()?);
    let title_run = &cols[0]["runs"][0];
    // Rows that open a page (album, artist, playlist) are cards even though their play
    // overlay names a first track.
    let opens_page = r["navigationEndpoint"].get("browseEndpoint").is_some();
    let video_id = str_of(&r["playlistItemData"]["videoId"])
        .or_else(|| str_of(&title_run["navigationEndpoint"]["watchEndpoint"]["videoId"]))
        .or_else(|| {
            str_of(&r["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]
                ["playNavigationEndpoint"]["watchEndpoint"]["videoId"])
        })
        .filter(|_| !opens_page);

    let rest: Vec<&Value> = cols.iter().skip(1).flat_map(|c| runs_of(c)).collect();

    match video_id {
        Some(video_id) => {
            let mut b = byline(&rest);
            if let Some(fixed) = r["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"].get("text") {
                if let Some(d) = parse_duration(&text(fixed)) {
                    b.duration = Some(d);
                }
            }
            let mut detail = b.detail;
            if let Some(kind) = b.kind.filter(|k| k != "Song") {
                detail.insert(0, kind);
            }
            Some(Item::Track(Track {
                video_id,
                title,
                artists: b.artists,
                album: b.album,
                duration: b.duration,
                thumbnail: thumbnail(&r["thumbnail"]),
                set_video_id: str_of(&r["playlistItemData"]["playlistSetVideoId"]),
                playable: r["musicItemRendererDisplayPolicy"].as_str()
                    != Some("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT"),
                like: like_status(&r["menu"]),
                detail: (!detail.is_empty()).then(|| detail.join(" • ")),
            }))
        }
        None => {
            let target = target(&r["navigationEndpoint"]).or_else(|| target(&title_run["navigationEndpoint"]))?;
            let round = matches!(&target, Target::Browse { page_type: PageType::Artist | PageType::Channel, .. });
            let subtitle = cols.iter().skip(1).map(|c| text(c)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" • ");
            Some(Item::Card(Card { title, subtitle, thumbnail: thumbnail(&r["thumbnail"]), target, round, wide: false }))
        }
    }
}

pub(crate) fn two_row_item(r: &Value) -> Option<Item> {
    let title = text(&r["title"]);
    let target = target(&r["navigationEndpoint"]).or_else(|| target(&r["title"]["runs"][0]["navigationEndpoint"]))?;
    let round = matches!(&target, Target::Browse { page_type: PageType::Artist | PageType::Channel, .. });
    let wide = r["aspectRatio"].as_str().is_some_and(|a| a.contains("16_9"));
    Some(Item::Card(Card {
        title,
        subtitle: text(&r["subtitle"]),
        thumbnail: thumbnail(&r["thumbnailRenderer"]),
        target,
        round,
        wide,
    }))
}

fn nav_button(r: &Value) -> Option<Item> {
    let target = target(&r["clickCommand"])?;
    Some(Item::Card(Card {
        title: text(&r["buttonText"]),
        subtitle: String::new(),
        thumbnail: None,
        target,
        round: false,
        wide: false,
    }))
}

pub(crate) fn panel_video(r: &Value) -> Option<Track> {
    let video_id = str_of(&r["videoId"]).or_else(|| str_of(&r["navigationEndpoint"]["watchEndpoint"]["videoId"]))?;
    let runs = runs_of(&r["longBylineText"]);
    let mut b = byline(&runs);
    if b.artists.is_empty() {
        let short = text(&r["shortBylineText"]);
        if !short.is_empty() {
            b.artists.push(Link { name: short, browse_id: None });
        }
    }
    Some(Track {
        video_id,
        title: text(&r["title"]),
        artists: b.artists,
        album: b.album,
        duration: parse_duration(&text(&r["lengthText"])),
        thumbnail: thumbnail(&r["thumbnail"]),
        set_video_id: None,
        playable: r.get("unplayableText").is_none(),
        like: like_status(&r["menu"]),
        detail: None,
    })
}

fn item(v: &Value) -> Option<Item> {
    if let Some(r) = v.get("musicResponsiveListItemRenderer") {
        list_item(r)
    } else if let Some(r) = v.get("musicTwoRowItemRenderer") {
        two_row_item(r)
    } else if let Some(r) = v.get("musicNavigationButtonRenderer") {
        nav_button(r)
    } else if let Some(r) = v.get("playlistPanelVideoRenderer") {
        panel_video(r).map(Item::Track)
    } else {
        None
    }
}

fn items(list: &Value) -> Vec<Item> {
    list.as_array().map(|a| a.iter().filter_map(item).collect()).unwrap_or_default()
}

fn continuation_token(v: &Value) -> Option<String> {
    str_of(&v["continuations"][0]["nextContinuationData"]["continuation"])
        .or_else(|| str_of(&v["continuations"][0]["nextRadioContinuationData"]["continuation"]))
        .or_else(|| {
            let mut out = vec![];
            find_all(v, "continuationCommand", &mut out);
            out.first().and_then(|c| str_of(&c["token"]))
        })
}

// ---------- sections & headers ----------

fn layout_for(items: &[Item]) -> Layout {
    if items.iter().any(|i| matches!(i, Item::Track(_))) { Layout::List } else { Layout::Grid }
}

/// Converts one entry of a `sectionListRenderer.contents` array. May update the header.
fn section(sec: &Value, header: &mut Option<Header>, page_cont: &mut Option<String>) -> Vec<Section> {
    let Some((key, r)) = sec.as_object().and_then(|m| m.iter().next()) else { return vec![] };
    match key.as_str() {
        "musicShelfRenderer" | "musicPlaylistShelfRenderer" => {
            let items = items(&r["contents"]);
            if key == "musicPlaylistShelfRenderer" || page_cont.is_none() {
                *page_cont = continuation_token(r).or(page_cont.take());
            }
            let more = target(&r["bottomEndpoint"]).or_else(|| target(&r["title"]["runs"][0]["navigationEndpoint"]));
            vec![Section { title: text(&r["title"]), layout: layout_for(&items), items, more }]
        }
        "musicCarouselShelfRenderer" => {
            let h = &r["header"]["musicCarouselShelfBasicHeaderRenderer"];
            let more = target(&h["moreContentButton"]["buttonRenderer"]["navigationEndpoint"])
                .or_else(|| target(&h["title"]["runs"][0]["navigationEndpoint"]));
            let title = text(&h["title"]);
            vec![Section { title, layout: Layout::Carousel, items: items(&r["contents"]), more }]
        }
        "gridRenderer" => {
            let items = items(&r["items"]);
            *page_cont = continuation_token(r).or(page_cont.take());
            vec![Section {
                title: text(&r["header"]["gridHeaderRenderer"]["title"]),
                layout: Layout::Grid,
                items,
                more: None,
            }]
        }
        "musicCardShelfRenderer" => {
            // Search "top result": the entity card plus a few of its songs.
            let mut out = vec![];
            if let Some(t) = target(&r["title"]["runs"][0]["navigationEndpoint"]).or_else(|| target(&r["onTap"])) {
                let round = matches!(&t, Target::Browse { page_type: PageType::Artist | PageType::Channel, .. });
                out.push(Section {
                    title: "Top result".into(),
                    layout: Layout::Grid,
                    items: vec![Item::Card(Card {
                        title: text(&r["title"]),
                        subtitle: text(&r["subtitle"]),
                        thumbnail: thumbnail(&r["thumbnail"]),
                        target: t,
                        round,
                        wide: false,
                    })],
                    more: None,
                });
            }
            let mut songs = items(&r["contents"]);
            // Songs under an artist card omit the artist ("Song • 3:07"); take it from the card.
            if let Some(Section { items, .. }) = out.first() {
                if let Some(Item::Card(Card { title, target: Target::Browse { id, page_type: PageType::Artist, .. }, .. })) = items.first() {
                    for s in &mut songs {
                        if let Item::Track(t) = s {
                            if t.artists.is_empty() {
                                t.artists.push(Link { name: title.clone(), browse_id: Some(id.clone()) });
                            }
                        }
                    }
                }
            }
            if !songs.is_empty() {
                out.push(Section { title: String::new(), layout: Layout::List, items: songs, more: None });
            }
            out
        }
        "musicResponsiveHeaderRenderer" => {
            *header = Some(responsive_header(r));
            vec![]
        }
        "musicDescriptionShelfRenderer" => {
            if let Some(h) = header.as_mut() {
                if h.description.is_empty() {
                    h.description = text(&r["description"]);
                }
            }
            vec![]
        }
        "itemSectionRenderer" => {
            let items = items(&r["contents"]);
            if items.is_empty() { vec![] } else { vec![Section { title: String::new(), layout: layout_for(&items), items, more: None }] }
        }
        _ => vec![],
    }
}

fn responsive_header(r: &Value) -> Header {
    let strapline = text(&r["straplineTextOne"]);
    let subtitle = text(&r["subtitle"]);
    let mut play = None;
    let mut eps = vec![];
    find_all(&r["buttons"], "watchEndpoint", &mut eps);
    if let Some(w) = eps.first() {
        play = Some(Target::Watch { video_id: str_of(&w["videoId"]), playlist_id: str_of(&w["playlistId"]), params: None });
    }
    Header {
        title: text(&r["title"]),
        subtitle: [strapline, subtitle].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" • "),
        second_subtitle: text(&r["secondSubtitle"]),
        description: text(&r["description"]["musicDescriptionShelfRenderer"]["description"]),
        thumbnail: thumbnail(&r["thumbnail"]),
        play,
        radio: None,
        playlist_id: None,
        round: false,
    }
}

fn top_header(v: &Value) -> Option<Header> {
    let h = &v["header"];
    if let Some(r) = h.get("musicImmersiveHeaderRenderer").or_else(|| h.get("musicVisualHeaderRenderer")) {
        let mut radio = None;
        if let Some(b) = r["startRadioButton"]["buttonRenderer"].get("navigationEndpoint") {
            radio = target(b);
        }
        let play = target(&r["playButton"]["buttonRenderer"]["navigationEndpoint"]);
        let thumb = thumbnail(&r["thumbnail"]).or_else(|| thumbnail(&r["foregroundThumbnail"]));
        return Some(Header {
            title: text(&r["title"]),
            subtitle: text(&r["monthlyListenerCount"]),
            second_subtitle: String::new(),
            description: text(&r["description"]),
            thumbnail: thumb,
            play,
            radio,
            playlist_id: None,
            round: true,
        });
    }
    if let Some(r) = h.get("musicDetailHeaderRenderer") {
        return Some(Header {
            title: text(&r["title"]),
            subtitle: text(&r["subtitle"]),
            second_subtitle: text(&r["secondSubtitle"]),
            description: text(&r["description"]),
            thumbnail: thumbnail(&r["thumbnail"]),
            ..Default::default()
        });
    }
    if let Some(r) = h.get("musicHeaderRenderer") {
        let title = text(&r["title"]);
        if !title.is_empty() {
            return Some(Header { title, ..Default::default() });
        }
    }
    None
}

/// Generic browse response → Page.
pub fn page(v: &Value, browse_id: &str) -> Page {
    let mut header = top_header(v);
    let mut sections = vec![];
    let mut cont = None;

    let mut lists = vec![];
    find_all(&v["contents"], "sectionListRenderer", &mut lists);
    for list in &lists {
        if let Some(contents) = list["contents"].as_array() {
            for sec in contents {
                sections.extend(section(sec, &mut header, &mut cont));
            }
        }
        if cont.is_none() {
            cont = continuation_token(list);
        }
    }

    // Editable (own) playlists wrap their header and expose the playlist id.
    let mut editable = vec![];
    find_all(&v["contents"], "musicEditablePlaylistDetailHeaderRenderer", &mut editable);
    if let Some(e) = editable.first() {
        let id = str_of(&e["editHeader"]["musicPlaylistEditHeaderRenderer"]["playlistId"]);
        if header.is_none() {
            let mut inner = vec![];
            find_all(&e["header"], "musicResponsiveHeaderRenderer", &mut inner);
            header = inner.first().map(|r| responsive_header(r));
        }
        if let Some(h) = header.as_mut() {
            h.playlist_id = id;
        }
    }

    // Playlist/album pages: "play" plays the whole list.
    if let Some(h) = header.as_mut() {
        if let Some(pl) = browse_id.strip_prefix("VL") {
            h.play = Some(Target::Watch { video_id: None, playlist_id: Some(pl.to_string()), params: None });
        } else if browse_id.starts_with("MPRE") {
            // Album tracks' watch endpoints carry the album's OLAK… playlist id.
            let mut w = vec![];
            find_all(&v["contents"], "watchEndpoint", &mut w);
            let album_playlist = w.iter().find_map(|e| str_of(&e["playlistId"]));
            if let Some(pl) = album_playlist {
                h.play = Some(Target::Watch { video_id: None, playlist_id: Some(pl), params: None });
            }
        }
    }

    sections.retain(|s| !s.items.is_empty());
    Page { header, sections, continuation: cont }
}

/// Search response: flat result rows grouped into Songs / Videos / Albums / Artists / Playlists.
pub fn search(v: &Value) -> Page {
    let raw = page(v, "");
    let mut out: Vec<Section> = vec![];
    let mut songs = vec![];
    let mut videos = vec![];
    let mut albums = vec![];
    let mut artists = vec![];
    let mut playlists = vec![];

    for s in raw.sections {
        if s.title == "Top result" {
            out.push(s);
            continue;
        }
        for item in s.items {
            match &item {
                Item::Track(t) => match t.detail.as_deref() {
                    Some(d) if d.starts_with("Episode") => {}
                    Some(d) if d.starts_with("Video") => videos.push(item),
                    _ => songs.push(item),
                },
                Item::Card(c) => match &c.target {
                    Target::Browse { page_type: PageType::Album, .. } => albums.push(item),
                    Target::Browse { page_type: PageType::Artist, .. } => artists.push(item),
                    Target::Browse { page_type: PageType::Playlist, .. } => playlists.push(item),
                    _ => {}
                },
            }
        }
    }
    for (title, layout, items) in [
        ("Songs", Layout::List, songs),
        ("Albums", Layout::Carousel, albums),
        ("Artists", Layout::Carousel, artists),
        ("Playlists", Layout::Carousel, playlists),
        ("Videos", Layout::List, videos),
    ] {
        if !items.is_empty() {
            out.push(Section { title: title.into(), layout, items, more: None });
        }
    }
    Page { header: None, sections: out, continuation: None }
}

/// Continuation response for a playlist or section list.
pub fn continuation(v: &Value) -> Page {
    let c = &v["continuationContents"];
    if let Some(r) = c.get("musicPlaylistShelfContinuation").or_else(|| c.get("musicShelfContinuation")) {
        let items = items(&r["contents"]);
        return Page {
            header: None,
            sections: vec![Section { title: String::new(), layout: Layout::List, items, more: None }],
            continuation: continuation_token(r),
        };
    }
    if let Some(r) = c.get("sectionListContinuation") {
        let mut header = None;
        let mut cont = None;
        let sections = r["contents"]
            .as_array()
            .map(|a| a.iter().flat_map(|s| section(s, &mut header, &mut cont)).collect())
            .unwrap_or_default();
        return Page { header: None, sections, continuation: continuation_token(r) };
    }
    if let Some(r) = c.get("gridContinuation") {
        return Page {
            header: None,
            sections: vec![Section { title: String::new(), layout: Layout::Grid, items: items(&r["items"]), more: None }],
            continuation: continuation_token(r),
        };
    }
    // New style: onResponseReceivedActions.appendContinuationItemsAction.continuationItems
    let mut appended = vec![];
    find_all(v, "continuationItems", &mut appended);
    if let Some(list) = appended.first() {
        let items = items(list);
        let cont = continuation_token(&serde_json::json!({ "x": list }));
        return Page {
            header: None,
            sections: vec![Section { title: String::new(), layout: layout_for(&items), items, more: None }],
            continuation: cont,
        };
    }
    Page::default()
}

/// `next` endpoint → play queue (+ lyrics/related tab ids).
pub fn queue(v: &Value) -> Queue {
    let mut panels = vec![];
    find_all(v, "playlistPanelRenderer", &mut panels);
    if panels.is_empty() {
        find_all(v, "playlistPanelContinuation", &mut panels);
    }
    let mut q = Queue::default();
    if let Some(p) = panels.first() {
        for entry in p["contents"].as_array().into_iter().flatten() {
            let r = entry
                .get("playlistPanelVideoRenderer")
                .or_else(|| entry["playlistPanelVideoWrapperRenderer"]["primaryRenderer"].get("playlistPanelVideoRenderer"));
            if let Some(t) = r.and_then(panel_video) {
                q.tracks.push(t);
            }
        }
        q.playlist_id = str_of(&p["playlistId"]);
        q.continuation = continuation_token(p);
    }
    let mut tabs = vec![];
    find_all(v, "tabRenderer", &mut tabs);
    for tab in tabs {
        let id = str_of(&tab["endpoint"]["browseEndpoint"]["browseId"]);
        match id.as_deref() {
            Some(i) if i.starts_with("MPLYt") => q.lyrics_browse_id = id,
            Some(i) if i.starts_with("FEmusic_related") || i.starts_with("MPTRt") => q.related_browse_id = id,
            _ => {}
        }
    }
    q
}

pub fn lyrics(v: &Value) -> Option<Lyrics> {
    let r = first(v, "musicDescriptionShelfRenderer")?;
    let text = text(&r["description"]);
    (!text.is_empty()).then(|| Lyrics { text, source: Some(self::text(&r["footer"])).filter(|s| !s.is_empty()) })
}
