//! UI-facing data model. Every browse response (home, explore, playlist, album, artist,
//! search, library) is reduced to one generic `Page`, so the UI needs a single renderer.

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum PageType {
    Album,
    Playlist,
    Artist,
    /// A plain YouTube channel (uploader that is not a music artist).
    Channel,
    Podcast,
    /// Mood/genre category or any other browse page.
    Other,
}

impl PageType {
    /// Best guess from the id alone (links inside track bylines carry no page type).
    pub fn from_id(browse_id: &str) -> Self {
        Self::from_api(None, browse_id)
    }

    pub(crate) fn from_api(page_type: Option<&str>, browse_id: &str) -> Self {
        match page_type {
            Some("MUSIC_PAGE_TYPE_ALBUM") | Some("MUSIC_PAGE_TYPE_AUDIOBOOK") => Self::Album,
            Some("MUSIC_PAGE_TYPE_PLAYLIST") => Self::Playlist,
            Some("MUSIC_PAGE_TYPE_ARTIST") => Self::Artist,
            Some("MUSIC_PAGE_TYPE_USER_CHANNEL") => Self::Channel,
            Some("MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE") => Self::Podcast,
            Some(_) => Self::Other,
            None if browse_id.starts_with("MPRE") => Self::Album,
            None if browse_id.starts_with("VL") => Self::Playlist,
            None if browse_id.starts_with("UC") => Self::Artist,
            None => Self::Other,
        }
    }
}

/// Where activating an item leads.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Browse { id: String, params: Option<String>, page_type: PageType },
    Watch { video_id: Option<String>, playlist_id: Option<String>, params: Option<String> },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub browse_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LikeStatus {
    Like,
    Dislike,
    Indifferent,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<Link>,
    pub album: Option<Link>,
    /// Seconds.
    pub duration: Option<u32>,
    pub thumbnail: Option<String>,
    /// Needed to remove the track from a playlist.
    pub set_video_id: Option<String>,
    /// False for deleted/region-blocked tracks (grey-out).
    pub playable: bool,
    pub like: Option<LikeStatus>,
    /// Extra text such as "33M plays" or "Video".
    pub detail: Option<String>,
}

impl Track {
    pub fn artist_line(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }
}

/// Album, playlist, artist, video tile or mood button.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Card {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
    pub target: Target,
    /// Artists and 16:9 videos are drawn differently.
    pub round: bool,
    pub wide: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum Item {
    Track(Track),
    Card(Card),
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Vertical track list.
    List,
    /// Horizontal row of cards (or a compact grid of tracks).
    Carousel,
    /// Wrapping grid of cards.
    Grid,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub layout: Layout,
    pub items: Vec<Item>,
    /// "See all" destination.
    pub more: Option<Target>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub second_subtitle: String,
    pub description: String,
    pub thumbnail: Option<String>,
    /// Playlist to play for the header's play button.
    pub play: Option<Target>,
    /// Radio/mix for artists.
    pub radio: Option<Target>,
    /// Set for playlists the user owns (editable).
    pub playlist_id: Option<String>,
    pub round: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub header: Option<Header>,
    pub sections: Vec<Section>,
    /// Token for loading more sections or tracks.
    pub continuation: Option<String>,
}

impl Page {
    /// All playable tracks on the page, in order (used for "play all").
    pub fn tracks(&self) -> Vec<Track> {
        self.sections
            .iter()
            .filter(|s| s.layout == Layout::List)
            .flat_map(|s| s.items.iter())
            .filter_map(|i| match i {
                Item::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Play queue as returned by the `next` endpoint.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub playlist_id: Option<String>,
    /// Endless radio continuation.
    pub continuation: Option<String>,
    pub lyrics_browse_id: Option<String>,
    pub related_browse_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Lyrics {
    pub text: String,
    pub source: Option<String>,
}
