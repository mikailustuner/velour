//! Messages the views send to the app.

use velour_ytm::{LikeStatus, Target, Track};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Home,
    Explore,
    Library(LibraryTab),
    Liked,
    History,
    Downloads,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryTab {
    Playlists,
    Albums,
    Artists,
}

#[derive(Debug)]
pub enum AppMsg {
    // ---- navigation ----
    Root(Root),
    Open(Target),
    Search(String),
    /// Search box text changed (for suggestions).
    SearchTyped(String),
    Back,
    Reload,
    LoadMore,

    // ---- playback ----
    PlayTracks {
        tracks: Vec<Track>,
        start: usize,
        shuffle: bool,
    },
    PlayTarget(Target),
    /// Resolve a stream ahead of time (pointer resting on a song or Play button).
    Prefetch(String),
    Radio(String),
    PlayNext(Track),
    AddToQueue(Track),
    TogglePlay,
    Next,
    Prev,
    Seek(f64),
    SeekRelative(f64),
    SetVolume(f64),
    VolumeStep(f64),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    JumpTo(usize),
    RemoveFromQueue(usize),
    MoveInQueue(usize, usize),
    Player(velour_player::Event),

    // ---- library ----
    Rate(Track, LikeStatus),
    RateCurrent,
    AddToPlaylist {
        playlist_id: String,
        track: Track,
    },
    NewPlaylist(Option<Track>),
    CreatePlaylist {
        title: String,
        tracks: Vec<Track>,
    },
    /// Keep only the current song in the queue.
    ClearQueue,
    /// Save the queue as a new playlist (title asked first).
    SaveQueue,
    RemoveFromPlaylist {
        playlist_id: String,
        track: Track,
    },
    RenamePlaylist {
        playlist_id: String,
        title: String,
    },
    DeletePlaylist(String),
    SaveToLibrary {
        playlist_id: String,
        save: bool,
    },
    Download(Vec<Track>),
    DeleteDownload(String),

    // ---- ui ----
    ToggleQueue,
    ToggleNowPlaying,
    /// Escape: closes the now-playing view if open.
    Escape,
    ToggleLyrics,
    LyricsTick,
    ToggleTheme,
    SetTheme(crate::settings::Theme),
    Settings(SettingChange),
    WindowWidth(i32),
    ShowWindow,
    TrayReady(crate::integrations::TrayHandle),
    CloseRequested,
    Quit,
}

#[derive(Debug, Clone)]
pub enum SettingChange {
    CloseToBackground(bool),
    Notifications(bool),
    Tray(bool),
    Discord(bool),
    DiscordAppId(String),
    Eq(String),
    DownloadDir(std::path::PathBuf),
    SendHistory(bool),
    LowMemory(bool),
    Browser(String),
    Downloads(bool),
    SidebarPlaylists(bool),
    ClearCache,
    UpdateYtDlp,
}
