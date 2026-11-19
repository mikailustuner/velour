//! Messages the views send to the app.

use sonora_ytm::{LikeStatus, Target, Track};

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
    Back,
    Reload,
    LoadMore,

    // ---- playback ----
    PlayTracks { tracks: Vec<Track>, start: usize, shuffle: bool },
    PlayTarget(Target),
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
    Player(sonora_player::Event),

    // ---- library ----
    Rate(Track, LikeStatus),
    RateCurrent,
    AddToPlaylist { playlist_id: String, track: Track },
    NewPlaylist(Option<Track>),
    CreatePlaylist { title: String, track: Option<Track> },
    RemoveFromPlaylist { playlist_id: String, track: Track },
    RenamePlaylist { playlist_id: String, title: String },
    DeletePlaylist(String),
    SaveToLibrary { playlist_id: String, save: bool },
    Download(Vec<Track>),
    DeleteDownload(String),

    // ---- ui ----
    ToggleQueue,
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
    ClearCache,
}
