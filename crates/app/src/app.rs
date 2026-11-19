//! Main window and application logic: navigation, loading, playback, library actions.

use crate::downloads::{self, Index};
use crate::icon_names as icons;
use crate::integrations::Integrations;
use crate::lyrics::{self, LyricsResult};
use crate::msg::{AppMsg, LibraryTab, Root, SettingChange};
use crate::page_view::{self, Ctx, PageView};
use crate::player_bar::PlayerBar;
use crate::queue::{PlayQueue, RadioSource};
use crate::settings::{Session, Settings, Theme};
use crate::side_panel::{Panel, SidePanel};
use crate::widgets::icon_button;
use crate::{eq, settings_page};
use gtk::glib;
use gtk::prelude::*;
use relm4::prelude::*;
use relm4::Sender;
use sonora_player::{Event, Player, Resolver};
use sonora_ytm::{Card, Client, Cookies, Item, LikeStatus, Page, PageType, Queue, Section, Target, Track};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
enum NavKey {
    Root(Root),
    Browse { id: String, params: Option<String> },
    Search(String),
}

struct NavEntry {
    key: NavKey,
    seq: u64,
    widget: gtk::Widget,
    view: Option<PageView>,
    continuation: Option<String>,
    loading_more: bool,
}

#[derive(Debug)]
pub enum CmdOut {
    Connected(Result<(Arc<Client>, String), String>),
    Page { seq: u64, result: Result<Page, String> },
    More { seq: u64, result: Result<Page, String> },
    Resolved { id: String, result: Result<String, String>, for_next: bool },
    QueueFetched { result: Result<Queue, String>, start_id: Option<String> },
    RadioMore(Result<Queue, String>),
    Playlists(Result<Page, String>),
    Lyrics { id: String, result: LyricsResult },
    Done { message: Option<String>, error: Option<String>, refresh_playlists: bool, reload: bool, back: bool },
    Downloaded { track: Track, result: Result<PathBuf, String> },
}

pub struct App {
    window: gtk::ApplicationWindow,
    input: Sender<AppMsg>,
    settings: Settings,
    ytm: Option<Arc<Client>>,
    connecting: bool,
    http: reqwest::Client,
    resolver: Arc<Resolver>,
    player: Option<Player>,
    downloads: Index,
    download_queue: VecDeque<Track>,
    downloading: bool,
    downloaded_count: usize,

    // navigation
    sidebar: gtk::Box,
    nav_list: gtk::ListBox,
    nav_labels: Vec<gtk::Label>,
    sidebar_extras: Vec<gtk::Widget>,
    search: gtk::SearchEntry,
    search_button: gtk::Button,
    playlists_list: gtk::ListBox,
    new_playlist_button: gtk::Button,
    content: gtk::Stack,
    back: gtk::Button,
    nav: Vec<NavEntry>,
    next_seq: u64,
    own_playlists: Rc<Vec<(String, String)>>,
    account: String,

    // playback
    queue: PlayQueue,
    loaded_id: Option<String>,
    pending_id: Option<String>,
    prepared_next: Option<String>,
    resolving_next: Option<String>,
    ended_eof: bool,
    paused: bool,
    volume: f64,
    muted: bool,
    position: f64,
    resume_at: Option<f64>,
    failures: u32,
    history_sent_for: Option<String>,
    lyrics_timer: Option<glib::SourceId>,
    radio_loading: bool,
    lyrics_for: Option<String>,

    bar: PlayerBar,
    panel: SidePanel,
    toast: gtk::Revealer,
    toast_label: gtk::Label,
    toast_timer: Option<glib::SourceId>,
    css: gtk::CssProvider,
    hold: Option<gtk::gio::ApplicationHoldGuard>,
    integrations: Integrations,
    collapsed: bool,
}

const NAV_ROOTS: [(Root, &str, &str); 6] = [
    (Root::Home, "Home", icons::HOME_SYMBOLIC),
    (Root::Explore, "Explore", icons::EXPLORE_SYMBOLIC),
    (Root::Library(LibraryTab::Playlists), "Library", icons::LIBRARY_MUSIC_SYMBOLIC),
    (Root::Liked, "Liked Music", icons::FAVORITE_SYMBOLIC),
    (Root::History, "History", icons::HISTORY_SYMBOLIC),
    (Root::Downloads, "Downloads", icons::DOWNLOAD_SYMBOLIC),
];

fn root_title(r: Root) -> &'static str {
    match r {
        Root::Home => "Home",
        Root::Explore => "Explore",
        Root::Library(_) => "Library",
        Root::Liked => "Liked Music",
        Root::History => "History",
        Root::Downloads => "Downloads",
        Root::Settings => "Settings",
    }
}

fn nav_index(r: Root) -> Option<usize> {
    NAV_ROOTS.iter().position(|(x, _, _)| match (x, r) {
        (Root::Library(_), Root::Library(_)) => true,
        (a, b) => *a == b,
    })
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

impl Component for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = CmdOut;
    type Root = gtk::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::ApplicationWindow::builder()
            .title("Sonora")
            .default_width(1200)
            .default_height(780)
            .css_classes(["sonora"])
            .build()
    }

    fn init(_: (), window: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let input = sender.input_sender().clone();
        let settings = Settings::load();

        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(&WidgetExt::display(&window), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        // ---------- sidebar ----------
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar.add_css_class("sidebar");
        sidebar.set_size_request(220, -1);
        // Children like the playlists header expand; the sidebar itself must not.
        sidebar.set_hexpand(false);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search"));
        search.update_property(&[gtk::accessible::Property::Label("Search YouTube Music")]);
        {
            let s = input.clone();
            search.connect_activate(move |e| {
                let q = e.text().trim().to_string();
                if !q.is_empty() {
                    s.emit(AppMsg::Search(q));
                }
            });
        }
        let search_button = icon_button(icons::SEARCH_SYMBOLIC, "Search (Ctrl+F)");
        search_button.set_visible(false);
        search_button.set_halign(gtk::Align::Center);
        {
            let s = input.clone();
            search_button.connect_clicked(move |b| {
                let s = s.clone();
                page_view::ask_text(b, "Search", "", move |q| s.emit(AppMsg::Search(q)));
            });
        }

        let nav_list = gtk::ListBox::new();
        nav_list.set_selection_mode(gtk::SelectionMode::Single);
        let mut nav_labels = vec![];
        for (_, title, icon) in NAV_ROOTS {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            b.append(&gtk::Image::from_icon_name(icon));
            let l = gtk::Label::new(Some(title));
            l.set_xalign(0.0);
            b.append(&l);
            nav_labels.push(l);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            row.set_tooltip_text(Some(title));
            row.update_property(&[gtk::accessible::Property::Label(title)]);
            nav_list.append(&row);
        }
        {
            let s = input.clone();
            nav_list.connect_row_activated(move |_, row| {
                if let Some((r, _, _)) = NAV_ROOTS.get(row.index() as usize) {
                    s.emit(AppMsg::Root(*r));
                }
            });
        }

        let playlists_header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let pl_label = gtk::Label::new(Some("PLAYLISTS"));
        pl_label.add_css_class("section-label");
        pl_label.set_xalign(0.0);
        pl_label.set_hexpand(true);
        let new_pl = icon_button(icons::ADD_SYMBOLIC, "New playlist");
        new_pl.add_css_class("dim");
        new_pl.set_valign(gtk::Align::End);
        {
            let s = input.clone();
            new_pl.connect_clicked(move |_| s.emit(AppMsg::NewPlaylist(None)));
        }
        playlists_header.append(&pl_label);
        playlists_header.append(&new_pl);
        let playlists_list = gtk::ListBox::new();
        playlists_list.set_selection_mode(gtk::SelectionMode::None);
        {
            let s = input.clone();
            playlists_list.connect_row_activated(move |_, row| {
                let id = row.widget_name().to_string();
                s.emit(AppMsg::Open(Target::Browse { id, params: None, page_type: PageType::Playlist }));
            });
        }
        let pl_scroll = gtk::ScrolledWindow::new();
        pl_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        pl_scroll.set_vexpand(true);
        pl_scroll.set_child(Some(&playlists_list));

        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let theme_button = icon_button(icons::DARK_MODE_SYMBOLIC, "Toggle dark mode");
        theme_button.add_css_class("dim");
        {
            let s = input.clone();
            theme_button.connect_clicked(move |_| s.emit(AppMsg::ToggleTheme));
        }
        let settings_button = icon_button(icons::SETTINGS_SYMBOLIC, "Settings (Ctrl+,)");
        settings_button.add_css_class("dim");
        {
            let s = input.clone();
            settings_button.connect_clicked(move |_| s.emit(AppMsg::Root(Root::Settings)));
        }
        bottom.append(&settings_button);
        bottom.append(&theme_button);

        let brand = gtk::Label::new(Some("Sonora"));
        brand.add_css_class("brand");
        brand.set_xalign(0.0);
        sidebar.append(&brand);
        sidebar.append(&search);
        sidebar.append(&search_button);
        sidebar.append(&nav_list);
        sidebar.append(&playlists_header);
        sidebar.append(&pl_scroll);
        sidebar.append(&bottom);
        let sidebar_extras: Vec<gtk::Widget> = vec![brand.upcast(), playlists_header.upcast(), pl_scroll.upcast()];

        // ---------- content ----------
        let back = icon_button(icons::ARROW_BACK_SYMBOLIC, "Back (Alt+←)");
        back.add_css_class("back");
        back.set_halign(gtk::Align::Start);
        back.set_valign(gtk::Align::Start);
        back.set_margin_top(12);
        back.set_margin_start(12);
        back.set_visible(false);
        {
            let s = input.clone();
            back.connect_clicked(move |_| s.emit(AppMsg::Back));
        }
        let content = gtk::Stack::new();
        content.set_hexpand(true);
        content.set_vexpand(true);
        content.set_transition_type(gtk::StackTransitionType::Crossfade);
        content.set_transition_duration(150);

        let toast_label = gtk::Label::new(None);
        toast_label.set_wrap(true);
        let toast_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        toast_box.add_css_class("toast");
        toast_box.append(&toast_label);
        let toast = gtk::Revealer::new();
        toast.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        toast.set_halign(gtk::Align::Center);
        toast.set_valign(gtk::Align::End);
        toast.set_margin_bottom(16);
        toast.set_child(Some(&toast_box));
        toast_label.set_accessible_role(gtk::AccessibleRole::Status);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&content));
        overlay.add_overlay(&back);
        overlay.add_overlay(&toast);

        let panel = SidePanel::new(&input);
        // Floating cards with 12 px gaps on the black canvas (Tiphys layout).
        let main_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        main_row.set_vexpand(true);
        main_row.append(&sidebar);
        main_row.append(&overlay);
        main_row.append(&panel.root);

        let bar = PlayerBar::new(&input);
        let layout = gtk::Box::new(gtk::Orientation::Vertical, 12);
        layout.add_css_class("shell");
        layout.append(&main_row);
        layout.append(&bar.root);
        window.set_child(Some(&layout));

        install_shortcuts(&window, &input, &search);
        // Don't start with the cursor in the search box (Space would type there).
        window.connect_map(|w| {
            let w = w.clone();
            glib::idle_add_local_once(move || GtkWindowExt::set_focus(&w, None::<&gtk::Widget>));
        });
        {
            let s = input.clone();
            window.connect_close_request(move |_| {
                s.emit(AppMsg::CloseRequested);
                glib::Propagation::Stop
            });
            let s = input.clone();
            window.connect_default_width_notify(move |w| s.emit(AppMsg::WindowWidth(w.default_width())));
        }

        // ---------- services ----------
        let profile = sonora_ytm::auth::default_firefox_profile().unwrap_or_default();
        let resolver = Arc::new(Resolver::new(profile));
        let player = {
            let s = input.clone();
            // SONORA_AO=null runs silently (used for testing).
            let ao = std::env::var("SONORA_AO").ok();
            match Player::new(ao.as_deref(), move |e| s.emit(AppMsg::Player(e))) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("sonora: audio unavailable: {e:#}");
                    None
                }
            }
        };
        let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().expect("http client");

        let mut model = App {
            window: window.clone(),
            input: input.clone(),
            ytm: None,
            connecting: false,
            http,
            resolver,
            player,
            downloads: Index::load(),
            download_queue: VecDeque::new(),
            downloading: false,
            downloaded_count: 0,
            sidebar,
            nav_list,
            nav_labels,
            sidebar_extras,
            search,
            search_button,
            playlists_list,
            new_playlist_button: new_pl.clone(),
            content,
            back,
            nav: vec![],
            next_seq: 1,
            own_playlists: Rc::new(vec![]),
            account: String::new(),
            queue: PlayQueue::default(),
            loaded_id: None,
            pending_id: None,
            prepared_next: None,
            resolving_next: None,
            ended_eof: false,
            paused: true,
            volume: settings.volume,
            muted: false,
            position: 0.0,
            resume_at: None,
            failures: 0,
            history_sent_for: None,
            lyrics_timer: None,
            radio_loading: false,
            lyrics_for: None,
            bar,
            panel,
            toast,
            toast_label,
            toast_timer: None,
            css,
            hold: None,
            integrations: Integrations::new(&input),
            collapsed: false,
            settings,
        };
        model.apply_theme();
        model.apply_eq();
        if let Some(p) = &model.player {
            let _ = p.set_volume(model.volume);
        }
        model.bar.set_volume(model.volume, false);
        model.restore_session();
        model.integrations.apply_settings(&model.settings);
        model.connect(&sender);
        model.navigate(NavKey::Root(Root::Home), true, &sender);
        crate::images::prune_disk_cache();
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: AppMsg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            // ---------------- navigation ----------------
            AppMsg::Root(r) => self.navigate(NavKey::Root(r), true, &sender),
            AppMsg::Open(Target::Browse { id, params, .. }) => self.navigate(NavKey::Browse { id, params }, false, &sender),
            AppMsg::Open(t @ Target::Watch { .. }) => self.play_target(t, &sender),
            AppMsg::Search(q) => {
                self.search.set_text(&q);
                self.navigate(NavKey::Search(q), false, &sender);
            }
            AppMsg::Back => self.go_back(),
            AppMsg::Reload => {
                if self.ytm.is_none() {
                    self.connect(&sender);
                }
                if let Some(key) = self.nav.last().map(|e| e.key.clone()) {
                    self.nav.pop().map(|e| self.content.remove(&e.widget));
                    self.navigate(key, false, &sender);
                }
            }
            AppMsg::LoadMore => self.load_more(&sender),

            // ---------------- playback ----------------
            AppMsg::PlayTracks { tracks, start, shuffle } => {
                if shuffle != self.queue.shuffle() {
                    self.queue.set_shuffle(shuffle);
                    self.bar.set_shuffle(shuffle);
                }
                self.queue.set(tracks, start);
                self.queue_changed();
                self.start_current(&sender);
            }
            AppMsg::PlayTarget(t) => self.play_target(t, &sender),
            AppMsg::Radio(id) => self.play_target(Target::Watch { video_id: Some(id.clone()), playlist_id: Some(format!("RDAMVM{id}")), params: Some("wAEB".into()) }, &sender),
            AppMsg::PlayNext(t) => {
                let empty = self.queue.is_empty();
                self.queue.insert_next(t.clone());
                self.queue_changed();
                if empty { self.start_current(&sender) } else { self.prepare_next(&sender) }
                self.toast(&format!("“{}” plays next", t.title));
            }
            AppMsg::AddToQueue(t) => {
                let empty = self.queue.is_empty();
                self.queue.append([t.clone()]);
                self.queue_changed();
                if empty { self.start_current(&sender) } else { self.prepare_next(&sender) }
                self.toast(&format!("Added “{}” to the queue", t.title));
            }
            AppMsg::TogglePlay => {
                if self.loaded_id.is_none() && self.pending_id.is_none() {
                    if self.queue.current().is_some() {
                        self.start_current(&sender);
                    }
                } else if let Some(p) = &self.player {
                    let _ = p.toggle_pause();
                }
            }
            AppMsg::Next => {
                if self.queue.advance() {
                    self.start_current(&sender);
                }
            }
            AppMsg::Prev => {
                if self.position > 3.0 || !self.queue.back() {
                    self.seek(0.0);
                } else {
                    self.start_current(&sender);
                }
            }
            AppMsg::Seek(s) => self.seek(s),
            AppMsg::SeekRelative(d) => self.seek((self.position + d).max(0.0)),
            AppMsg::SetVolume(v) => {
                self.volume = v;
                self.muted = false;
                if let Some(p) = &self.player {
                    let _ = p.set_mute(false);
                    let _ = p.set_volume(v);
                }
                self.bar.set_volume(v, false);
                self.settings.volume = v;
            }
            AppMsg::VolumeStep(d) => sender.input(AppMsg::SetVolume((self.volume + d).clamp(0.0, 100.0))),
            AppMsg::ToggleMute => {
                self.muted = !self.muted;
                if let Some(p) = &self.player {
                    let _ = p.set_mute(self.muted);
                }
                self.bar.set_volume(self.volume, self.muted);
            }
            AppMsg::ToggleShuffle => {
                let on = !self.queue.shuffle();
                self.queue.set_shuffle(on);
                self.bar.set_shuffle(on);
                self.queue_changed();
                self.prepare_next(&sender);
                self.integrations.shuffle_repeat_changed(on, self.queue.repeat);
            }
            AppMsg::CycleRepeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                self.bar.set_repeat(self.queue.repeat);
                self.prepare_next(&sender);
                self.integrations.shuffle_repeat_changed(self.queue.shuffle(), self.queue.repeat);
            }
            AppMsg::JumpTo(i) => {
                if self.queue.jump(i) {
                    self.start_current(&sender);
                }
            }
            AppMsg::RemoveFromQueue(i) => {
                let was_current = self.queue.remove(i);
                self.queue_changed();
                if was_current {
                    if self.queue.is_empty() { self.stop() } else { self.start_current(&sender) }
                } else {
                    self.prepare_next(&sender);
                }
            }
            AppMsg::MoveInQueue(from, to) => {
                self.queue.move_item(from, to);
                self.queue_changed();
                self.prepare_next(&sender);
            }
            AppMsg::Player(e) => self.on_player(e, &sender),

            // ---------------- library ----------------
            AppMsg::Rate(t, status) => self.rate(t.video_id.clone(), status, &sender),
            AppMsg::RateCurrent => {
                if let Some(t) = self.queue.current() {
                    let status = if t.like == Some(LikeStatus::Like) { LikeStatus::Indifferent } else { LikeStatus::Like };
                    self.rate(t.video_id.clone(), status, &sender);
                }
            }
            AppMsg::AddToPlaylist { playlist_id, track } => {
                let name = self.own_playlists.iter().find(|(id, _)| *id == playlist_id).map(|(_, t)| t.clone()).unwrap_or_default();
                self.api(&sender, false, false, move |c| async move {
                    c.add_to_playlist(&playlist_id, &[&track.video_id]).await.map(|_| format!("Added to {name}"))
                });
            }
            AppMsg::NewPlaylist(track) => {
                let s = self.input.clone();
                // Anchored on the sidebar "+" (or the search button when collapsed).
                let anchor: gtk::Widget = if self.collapsed { self.search_button.clone().upcast() } else { self.new_playlist_button.clone().upcast() };
                page_view::ask_text(&anchor, "New playlist", "", move |title| s.emit(AppMsg::CreatePlaylist { title, track: track.clone() }));
            }
            AppMsg::CreatePlaylist { title, track } => {
                self.api(&sender, true, false, move |c| async move {
                    let ids: Vec<&str> = track.iter().map(|t| t.video_id.as_str()).collect();
                    c.create_playlist(&title, &ids).await.map(|_| format!("Created “{title}”"))
                });
            }
            AppMsg::RemoveFromPlaylist { playlist_id, track } => {
                let Some(set_id) = track.set_video_id.clone() else { return };
                self.api(&sender, false, true, move |c| async move {
                    c.remove_from_playlist(&playlist_id, &track.video_id, &set_id).await.map(|_| format!("Removed “{}”", track.title))
                });
            }
            AppMsg::RenamePlaylist { playlist_id, title } => {
                self.api(&sender, true, true, move |c| async move { c.rename_playlist(&playlist_id, &title).await.map(|_| "Playlist renamed".to_string()) });
            }
            AppMsg::DeletePlaylist(id) => {
                sender.oneshot_command({
                    let c = self.ytm.clone();
                    async move {
                        let r = match c {
                            Some(c) => c.delete_playlist(&id).await.map_err(err),
                            None => Err("Not signed in".into()),
                        };
                        CmdOut::Done { message: r.as_ref().ok().map(|_| "Playlist deleted".into()), error: r.err(), refresh_playlists: true, reload: false, back: true }
                    }
                });
            }
            AppMsg::SaveToLibrary { playlist_id, save } => {
                self.api(&sender, true, false, move |c| async move {
                    c.rate_playlist(&playlist_id, save).await.map(|_| if save { "Saved to library".into() } else { "Removed from library".into() })
                });
            }
            AppMsg::Download(tracks) => {
                let new: Vec<Track> = tracks.into_iter().filter(|t| self.downloads.file(&t.video_id).is_none() && !self.download_queue.iter().any(|q| q.video_id == t.video_id)).collect();
                if new.is_empty() {
                    self.toast("Already downloaded");
                    return;
                }
                self.toast(&format!("Downloading {} song{}…", new.len(), if new.len() == 1 { "" } else { "s" }));
                self.download_queue.extend(new);
                self.next_download(&sender);
            }
            AppMsg::DeleteDownload(id) => {
                match self.downloads.remove(&id) {
                    Ok(()) => self.toast("Download deleted"),
                    Err(e) => self.toast(&err(e)),
                }
                self.reload_if(|k| *k == NavKey::Root(Root::Downloads), &sender);
            }

            // ---------------- ui ----------------
            AppMsg::ToggleQueue => self.toggle_panel(Panel::Queue, &sender),
            AppMsg::ToggleLyrics => self.toggle_panel(Panel::Lyrics, &sender),
            AppMsg::LyricsTick => {
                if let (Some(p), false) = (&self.player, self.paused) {
                    self.panel.set_position(p.position());
                }
            }
            AppMsg::ToggleTheme => {
                self.settings.theme = if self.is_dark() { Theme::Light } else { Theme::Dark };
                self.apply_theme();
                self.settings.save();
            }
            AppMsg::SetTheme(t) => {
                self.settings.theme = t;
                self.apply_theme();
                self.settings.save();
            }
            AppMsg::Settings(change) => self.change_setting(change, &sender),
            AppMsg::WindowWidth(w) => self.set_collapsed(w < 900),
            AppMsg::ShowWindow => self.window.present(),
            AppMsg::TrayReady(h) => self.integrations.tray_ready(h),
            AppMsg::CloseRequested => {
                let playing = self.loaded_id.is_some() && !self.paused;
                if self.settings.close_to_background && playing {
                    if self.hold.is_none() {
                        self.hold = Some(relm4::main_application().hold());
                    }
                    self.window.set_visible(false);
                } else {
                    self.quit();
                }
            }
            AppMsg::Quit => self.quit(),
        }
    }

    fn update_cmd(&mut self, msg: CmdOut, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            CmdOut::Connected(result) => {
                self.connecting = false;
                match result {
                    Ok((c, account)) => {
                        self.ytm = Some(c);
                        self.account = account;
                        self.refresh_playlists(&sender);
                        // Pages opened before sign-in finished are still waiting.
                        if let Some(e) = self.nav.last() {
                            if e.view.is_none() {
                                let key = e.key.clone();
                                self.nav.pop().map(|e| self.content.remove(&e.widget));
                                self.navigate(key, false, &sender);
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("Couldn’t sign in to YouTube Music: {e}\nMake sure you’re logged in to music.youtube.com in Firefox.");
                        let waiting = self.nav.last().filter(|e| e.view.is_none() && !matches!(e.key, NavKey::Root(Root::Downloads | Root::Settings))).map(|e| e.key.clone());
                        if let Some(key) = waiting {
                            let w = page_view::error_view(&self.title_for(&key), &msg, &self.input);
                            self.replace_widget(w);
                        }
                    }
                }
            }
            CmdOut::Page { seq, result } => {
                let Some(pos) = self.nav.iter().position(|e| e.seq == seq) else { return };
                let key = self.nav[pos].key.clone();
                let title = self.title_for(&key);
                let widget: gtk::Widget = match result {
                    Ok(page) => {
                        let editable = page.header.as_ref().and_then(|h| h.playlist_id.clone());
                        let top = match key {
                            NavKey::Root(Root::Library(tab)) => Some(self.library_tabs(tab)),
                            _ => None,
                        };
                        let view = PageView::new(&page, &title, top, self.ctx(editable));
                        let w: gtk::Widget = view.root.clone().upcast();
                        self.nav[pos].view = Some(view);
                        self.nav[pos].continuation = page.continuation;
                        w
                    }
                    Err(e) => page_view::error_view(&title, &e, &self.input),
                };
                self.set_entry_widget(pos, widget);
            }
            CmdOut::More { seq, result } => {
                let Some(entry) = self.nav.iter_mut().find(|e| e.seq == seq) else { return };
                entry.loading_more = false;
                match result {
                    Ok(page) => {
                        entry.continuation = page.continuation.clone();
                        if let Some(v) = entry.view.as_mut() {
                            v.append(&page);
                        }
                    }
                    Err(e) => {
                        entry.continuation = None;
                        self.toast(&format!("Couldn’t load more: {e}"));
                    }
                }
            }
            CmdOut::Resolved { id, result, for_next } => self.on_resolved(id, result, for_next, &sender),
            CmdOut::QueueFetched { result, start_id } => match result {
                Ok(q) if !q.tracks.is_empty() => {
                    let start = start_id.and_then(|id| q.tracks.iter().position(|t| t.video_id == id)).unwrap_or(0);
                    let radio = match (&q.continuation, &q.playlist_id) {
                        (Some(token), Some(pl)) => Some(RadioSource { token: token.clone(), playlist_id: pl.clone() }),
                        _ => None,
                    };
                    self.queue.set(q.tracks, start);
                    self.queue.radio = radio;
                    self.queue_changed();
                    self.start_current(&sender);
                }
                Ok(_) => self.toast("Nothing to play here"),
                Err(e) => self.toast(&format!("Couldn’t start playback: {e}")),
            },
            CmdOut::RadioMore(result) => {
                self.radio_loading = false;
                if let Ok(q) = result {
                    let known: HashSet<String> = self.queue.tracks().iter().map(|t| t.video_id.clone()).collect();
                    self.queue.radio = match (q.continuation, self.queue.radio.take()) {
                        (Some(token), Some(old)) => Some(RadioSource { token, playlist_id: old.playlist_id }),
                        _ => None,
                    };
                    self.queue.append(q.tracks.into_iter().filter(|t| !known.contains(&t.video_id)));
                    self.queue_changed();
                    self.prepare_next(&sender);
                }
            }
            CmdOut::Playlists(result) => {
                if let Ok(page) = result {
                    self.set_playlists(&page);
                }
            }
            CmdOut::Lyrics { id, result } => {
                if self.queue.current().map(|t| t.video_id.as_str()) == Some(id.as_str()) {
                    self.panel.set_lyrics(&result);
                    self.panel.set_position(self.position);
                }
            }
            CmdOut::Done { message, error, refresh_playlists, reload, back } => {
                if let Some(e) = error {
                    self.toast(&e);
                    return;
                }
                if let Some(m) = message {
                    self.toast(&m);
                }
                if refresh_playlists {
                    self.refresh_playlists(&sender);
                }
                if back {
                    self.go_back();
                }
                if reload {
                    sender.input(AppMsg::Reload);
                }
            }
            CmdOut::Downloaded { track, result } => {
                self.downloading = false;
                match result {
                    Ok(path) => {
                        self.downloads.entries.insert(track.video_id.clone(), downloads::Entry { path, track });
                        self.downloads.save();
                        self.downloaded_count += 1;
                    }
                    Err(e) => self.toast(&format!("Download failed for “{}”: {e}", track.title)),
                }
                if self.download_queue.is_empty() {
                    if self.downloaded_count > 0 {
                        self.toast(&format!("Downloaded {} song{}", self.downloaded_count, if self.downloaded_count == 1 { "" } else { "s" }));
                    }
                    self.downloaded_count = 0;
                    self.reload_if(|k| *k == NavKey::Root(Root::Downloads), &sender);
                } else {
                    self.next_download(&sender);
                }
            }
        }
    }
}

// =====================================================================
impl App {
    // ---------------- connection & data ----------------

    fn connect(&mut self, sender: &ComponentSender<Self>) {
        if self.connecting || self.ytm.is_some() {
            return;
        }
        self.connecting = true;
        sender.oneshot_command(async {
            let r = async {
                let c = Arc::new(Client::new(Cookies::from_firefox()?).await?);
                let account = c.account_name().await.unwrap_or_default();
                anyhow::Ok((c, account))
            }
            .await;
            CmdOut::Connected(r.map_err(err))
        });
    }

    fn refresh_playlists(&self, sender: &ComponentSender<Self>) {
        let Some(c) = self.ytm.clone() else { return };
        sender.oneshot_command(async move { CmdOut::Playlists(c.library_playlists().await.map_err(err)) });
    }

    fn set_playlists(&mut self, page: &Page) {
        while let Some(c) = self.playlists_list.first_child() {
            self.playlists_list.remove(&c);
        }
        let mut own = vec![];
        for item in page.sections.iter().flat_map(|s| &s.items) {
            let Item::Card(Card { title, subtitle, target: Target::Browse { id, page_type: PageType::Playlist, .. }, .. }) = item else { continue };
            if id == "VLLM" || id == "VLSE" {
                continue;
            }
            if !self.account.is_empty() && subtitle.contains(&self.account) {
                own.push((id.trim_start_matches("VL").to_string(), title.clone()));
            }
            let row = gtk::ListBoxRow::new();
            let l = gtk::Label::new(Some(title));
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row.set_child(Some(&l));
            row.set_tooltip_text(Some(&format!("{title}\n{subtitle}")));
            // The row name carries the browse id for the activation handler.
            row.set_widget_name(id);
            self.playlists_list.append(&row);
        }
        self.own_playlists = Rc::new(own);
    }

    fn ctx(&self, editable: Option<String>) -> Ctx {
        Ctx {
            sender: self.input.clone(),
            editable_playlist: editable,
            own_playlists: self.own_playlists.clone(),
            downloaded: Rc::new(self.downloads.entries.keys().cloned().collect()),
            playing: self.queue.current().map(|t| t.video_id.clone()),
        }
    }

    /// Runs an API call that ends in a toast.
    fn api<F, Fut>(&self, sender: &ComponentSender<Self>, refresh_playlists: bool, reload: bool, f: F)
    where
        F: FnOnce(Arc<Client>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<String>> + Send,
    {
        let c = self.ytm.clone();
        sender.oneshot_command(async move {
            let r = match c {
                Some(c) => f(c).await.map_err(err),
                None => Err("Not signed in yet".to_string()),
            };
            match r {
                Ok(m) => CmdOut::Done { message: Some(m), error: None, refresh_playlists, reload, back: false },
                Err(e) => CmdOut::Done { message: None, error: Some(e), refresh_playlists: false, reload: false, back: false },
            }
        });
    }

    fn rate(&mut self, video_id: String, status: LikeStatus, sender: &ComponentSender<Self>) {
        // Optimistic update of queue + bar.
        let like = Some(status);
        self.queue_set_like(&video_id, like);
        if self.queue.current().map(|t| t.video_id.as_str()) == Some(video_id.as_str()) {
            self.bar.set_like(like);
        }
        let liked_page = self.nav.last().is_some_and(|e| e.key == NavKey::Root(Root::Liked));
        self.api(sender, false, liked_page, move |c| async move {
            c.rate_song(&video_id, status).await.map(|_| match status {
                LikeStatus::Like => "Added to Liked Music".into(),
                _ => "Removed from Liked Music".into(),
            })
        });
    }

    fn queue_set_like(&mut self, video_id: &str, like: Option<LikeStatus>) {
        self.queue.update(video_id, |t| t.like = like);
    }

    // ---------------- navigation ----------------

    fn title_for(&self, key: &NavKey) -> String {
        match key {
            NavKey::Root(r) => root_title(*r).to_string(),
            NavKey::Browse { .. } => String::new(),
            NavKey::Search(q) => format!("Results for “{q}”"),
        }
    }

    fn navigate(&mut self, key: NavKey, reset: bool, sender: &ComponentSender<Self>) {
        if reset {
            for e in self.nav.drain(..) {
                self.content.remove(&e.widget);
            }
        } else if self.nav.last().is_some_and(|e| e.key == key && e.view.is_some()) {
            return;
        }
        let title = self.title_for(&key);
        let seq = self.next_seq;
        self.next_seq += 1;

        // Local pages render immediately.
        let local: Option<gtk::Widget> = match &key {
            NavKey::Root(Root::Settings) => Some(settings_page::build(&self.settings, &self.input, &self.account)),
            _ => None,
        };
        let widget = local.clone().unwrap_or_else(|| page_view::loading_view(&title));
        self.content.add_named(&widget, Some(&seq.to_string()));
        self.content.set_visible_child(&widget);
        self.nav.push(NavEntry { key: key.clone(), seq, widget, view: None, continuation: None, loading_more: false });
        self.sync_nav_ui(&key);
        if local.is_some() {
            return;
        }

        if key == NavKey::Root(Root::Downloads) {
            let tracks = self.downloads.tracks();
            let page = Page {
                header: None,
                sections: if tracks.is_empty() { vec![] } else {
                    vec![Section { title: String::new(), layout: sonora_ytm::Layout::List, items: tracks.into_iter().map(Item::Track).collect(), more: None }]
                },
                continuation: None,
            };
            sender.oneshot_command(async move { CmdOut::Page { seq, result: Ok(page) } });
            return;
        }
        let Some(c) = self.ytm.clone() else {
            // Shown as loading; filled in once sign-in finishes.
            self.connect(sender);
            return;
        };
        sender.oneshot_command(async move {
            let result = match key {
                NavKey::Root(Root::Home) => c.home().await,
                NavKey::Root(Root::Explore) => c.explore().await,
                NavKey::Root(Root::Liked) => c.liked_songs().await,
                NavKey::Root(Root::History) => c.history().await,
                NavKey::Root(Root::Library(LibraryTab::Playlists)) => c.library_playlists().await,
                NavKey::Root(Root::Library(LibraryTab::Albums)) => c.library_albums().await,
                NavKey::Root(Root::Library(LibraryTab::Artists)) => c.library_artists().await,
                NavKey::Root(Root::Downloads | Root::Settings) => unreachable!(),
                NavKey::Browse { id, params } => c.page(&id, params.as_deref()).await,
                NavKey::Search(q) => c.search(&q).await,
            };
            CmdOut::Page { seq, result: result.map_err(err) }
        });
    }

    fn sync_nav_ui(&self, key: &NavKey) {
        self.back.set_visible(self.nav.len() > 1);
        match key {
            NavKey::Root(r) => match nav_index(*r) {
                Some(i) => self.nav_list.select_row(self.nav_list.row_at_index(i as i32).as_ref()),
                None => self.nav_list.unselect_all(),
            },
            // Inside a playlist/album/search the sidebar shouldn't claim a section.
            _ => self.nav_list.unselect_all(),
        }
    }

    fn go_back(&mut self) {
        if self.nav.len() < 2 {
            return;
        }
        if let Some(e) = self.nav.pop() {
            let w = e.widget.clone();
            // Remove after the cross-fade so it doesn't flash.
            let content = self.content.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || content.remove(&w));
        }
        if let Some(e) = self.nav.last() {
            self.content.set_visible_child(&e.widget);
            let key = e.key.clone();
            self.sync_nav_ui(&key);
        }
        let playing = self.queue.current().map(|t| t.video_id.clone());
        if let Some(v) = self.nav.last_mut().and_then(|e| e.view.as_mut()) {
            v.set_playing(playing.as_deref());
        }
    }

    fn set_entry_widget(&mut self, pos: usize, widget: gtk::Widget) {
        let old = std::mem::replace(&mut self.nav[pos].widget, widget.clone());
        let name = self.nav[pos].seq.to_string();
        self.content.remove(&old);
        self.content.add_named(&widget, Some(&name));
        if pos + 1 == self.nav.len() {
            self.content.set_visible_child(&widget);
        }
    }

    fn replace_widget(&mut self, widget: gtk::Widget) {
        if !self.nav.is_empty() {
            let last = self.nav.len() - 1;
            self.set_entry_widget(last, widget);
        }
    }

    fn reload_if(&mut self, pred: impl Fn(&NavKey) -> bool, sender: &ComponentSender<Self>) {
        if self.nav.last().is_some_and(|e| pred(&e.key)) {
            sender.input(AppMsg::Reload);
        }
    }

    fn load_more(&mut self, sender: &ComponentSender<Self>) {
        let Some(c) = self.ytm.clone() else { return };
        let Some(e) = self.nav.last_mut() else { return };
        if e.loading_more {
            return;
        }
        let Some(token) = e.continuation.clone() else { return };
        e.loading_more = true;
        let seq = e.seq;
        sender.oneshot_command(async move { CmdOut::More { seq, result: c.more(&token).await.map_err(err) } });
    }

    fn library_tabs(&self, active: LibraryTab) -> gtk::Widget {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        b.add_css_class("segmented");
        b.set_halign(gtk::Align::Start);
        let mut group: Option<gtk::ToggleButton> = None;
        for (tab, label) in [(LibraryTab::Playlists, "Playlists"), (LibraryTab::Albums, "Albums"), (LibraryTab::Artists, "Artists")] {
            let t = gtk::ToggleButton::with_label(label);
            t.set_active(tab == active);
            if let Some(g) = &group {
                t.set_group(Some(g));
            } else {
                group = Some(t.clone());
            }
            let s = self.input.clone();
            t.connect_toggled(move |t| {
                if t.is_active() && tab != active {
                    s.emit(AppMsg::Root(Root::Library(tab)));
                }
            });
            b.append(&t);
        }
        b.upcast()
    }

    fn set_collapsed(&mut self, collapsed: bool) {
        if collapsed == self.collapsed {
            return;
        }
        self.collapsed = collapsed;
        self.sidebar.set_size_request(if collapsed { 64 } else { 220 }, -1);
        for l in &self.nav_labels {
            l.set_visible(!collapsed);
        }
        for w in &self.sidebar_extras {
            w.set_visible(!collapsed);
        }
        self.search.set_visible(!collapsed);
        self.search_button.set_visible(collapsed);
        if collapsed { self.sidebar.add_css_class("collapsed") } else { self.sidebar.remove_css_class("collapsed") }
    }

    // ---------------- playback ----------------

    fn play_target(&mut self, t: Target, sender: &ComponentSender<Self>) {
        let Target::Watch { video_id, playlist_id, params } = t else {
            return;
        };
        let Some(c) = self.ytm.clone() else {
            self.toast("Still signing in…");
            return;
        };
        let start_id = video_id.clone();
        sender.oneshot_command(async move {
            let result = c.queue(video_id.as_deref(), playlist_id.as_deref(), params.as_deref()).await.map_err(err);
            CmdOut::QueueFetched { result, start_id }
        });
    }

    /// Local file or a still-valid cached stream URL for `id`.
    fn source_for(&self, id: &str) -> Option<String> {
        self.downloads.file(id).map(|p| p.to_string_lossy().into_owned()).or_else(|| self.resolver.cached(id))
    }

    fn start_current(&mut self, sender: &ComponentSender<Self>) {
        let Some(t) = self.queue.current().cloned() else {
            self.stop();
            return;
        };
        self.bar.set_track(Some(&t));
        self.bar.set_loading(true);
        self.mark_playing(Some(&t.video_id));
        self.panel.set_queue_position(self.queue.position());
        self.pending_id = Some(t.video_id.clone());
        self.prepared_next = None;
        self.position = 0.0;
        self.integrations.track_changed(Some(&t));
        if self.panel.shown == Some(Panel::Lyrics) {
            self.load_lyrics(sender);
        }
        match self.source_for(&t.video_id) {
            Some(src) => self.on_resolved(t.video_id.clone(), Ok(src), false, sender),
            None => self.resolve(t.video_id.clone(), false, sender),
        }
    }

    fn resolve(&self, id: String, for_next: bool, sender: &ComponentSender<Self>) {
        let r = self.resolver.clone();
        sender.spawn_oneshot_command(move || {
            let result = r.resolve(&id).map_err(err);
            CmdOut::Resolved { id, result, for_next }
        });
    }

    fn on_resolved(&mut self, id: String, result: Result<String, String>, for_next: bool, sender: &ComponentSender<Self>) {
        if for_next {
            if self.resolving_next.as_deref() == Some(id.as_str()) {
                self.resolving_next = None;
            }
            if let (Ok(url), Some(next)) = (&result, self.queue.next()) {
                if next.video_id == id && self.loaded_id.is_some() {
                    if let Some(p) = &self.player {
                        if p.set_next(url, &id).is_ok() {
                            self.prepared_next = Some(id);
                        }
                    }
                }
            }
            return;
        }
        if self.pending_id.as_deref() != Some(id.as_str()) {
            return; // user moved on meanwhile
        }
        match result {
            Ok(url) => {
                self.failures = 0;
                match &self.player {
                    Some(p) => {
                        if let Err(e) = p.play(&url, &id) {
                            self.toast(&format!("Playback error: {e:#}"));
                        }
                    }
                    None => self.toast("Audio output isn’t available (libmpv failed to start)."),
                }
            }
            Err(e) => {
                self.failures += 1;
                let title = self.queue.current().map(|t| t.title.clone()).unwrap_or_default();
                self.toast(&format!("Can’t play “{title}”: {e}"));
                self.pending_id = None;
                self.bar.set_loading(false);
                // Skip to the next song, but don't spin through a broken queue.
                if self.failures < 3 && self.queue.advance() {
                    self.start_current(sender);
                } else {
                    self.failures = 0;
                    self.bar.set_paused(true);
                }
            }
        }
    }

    /// Prepares the following track in mpv so the switch is gapless.
    fn prepare_next(&mut self, sender: &ComponentSender<Self>) {
        if self.loaded_id.is_none() {
            return;
        }
        let Some(next) = self.queue.next().cloned() else {
            if let Some(p) = &self.player {
                let _ = p.clear_next();
            }
            self.prepared_next = None;
            return;
        };
        if self.prepared_next.as_deref() == Some(next.video_id.as_str()) && next.video_id != self.loaded_id.clone().unwrap_or_default() {
            return;
        }
        match self.source_for(&next.video_id) {
            Some(src) => {
                if let Some(p) = &self.player {
                    if p.set_next(&src, &next.video_id).is_ok() {
                        self.prepared_next = Some(next.video_id);
                    }
                }
            }
            None => {
                if let Some(p) = &self.player {
                    let _ = p.clear_next();
                }
                self.prepared_next = None;
                if self.resolving_next.as_deref() != Some(next.video_id.as_str()) {
                    self.resolving_next = Some(next.video_id.clone());
                    self.resolve(next.video_id, true, sender);
                }
            }
        }
    }

    fn on_player(&mut self, e: Event, sender: &ComponentSender<Self>) {
        match e {
            Event::Started { id } => {
                self.ended_eof = false;
                if self.queue.current().map(|t| t.video_id.as_str()) != Some(id.as_str()) {
                    // mpv moved on to the prepared next entry by itself.
                    if self.queue.next().map(|t| t.video_id.as_str()) == Some(id.as_str()) {
                        self.queue.advance();
                    }
                    if let Some(t) = self.queue.current().cloned() {
                        self.bar.set_track(Some(&t));
                        self.mark_playing(Some(&t.video_id));
                        self.panel.set_queue_position(self.queue.position());
                        self.integrations.track_changed(Some(&t));
                        if self.panel.shown == Some(Panel::Lyrics) {
                            self.load_lyrics(sender);
                        }
                    }
                }
                self.loaded_id = Some(id.clone());
                self.pending_id = None;
                self.prepared_next = None;
                self.bar.set_loading(false);
                self.bar.set_paused(self.paused);
                self.integrations.playback_changed(!self.paused);
                if let Some(at) = self.resume_at.take() {
                    self.seek(at);
                }
                self.prepare_next(sender);
                self.maybe_more_radio(sender);
                Session { queue: self.queue.clone(), position: 0.0 }.save();
            }
            Event::Position(p) => {
                self.position = p;
                self.maybe_send_history();
                self.bar.set_position(p);
                self.panel.set_position(p);
                self.integrations.position_changed(p);
            }
            Event::Duration(d) => {
                self.bar.set_duration(d);
                self.integrations.duration_known(d);
            }
            Event::Paused(p) => {
                self.paused = p;
                // mpv reports "not paused" while idle; only show Pause when something plays.
                self.bar.set_paused(p || (self.loaded_id.is_none() && self.pending_id.is_none()));
                self.integrations.playback_changed(!p && self.loaded_id.is_some());
            }
            Event::Volume(v) => {
                self.integrations.volume_changed(v);
            }
            Event::Ended { eof } => self.ended_eof = eof,
            Event::Idle => {
                if std::mem::take(&mut self.ended_eof) {
                    self.loaded_id = None;
                    if self.queue.advance() {
                        self.start_current(sender);
                    } else {
                        self.paused = true;
                        self.bar.set_paused(true);
                        self.integrations.playback_changed(false);
                    }
                }
            }
            Event::Error(e) => eprintln!("sonora: playback error: {e}"),
        }
    }

    fn seek(&mut self, s: f64) {
        if let Some(p) = &self.player {
            if self.loaded_id.is_some() {
                let _ = p.seek(s);
                self.position = s;
                self.bar.set_position(s);
                self.integrations.seeked(s);
            }
        }
    }

    fn stop(&mut self) {
        if let Some(p) = &self.player {
            let _ = p.stop();
        }
        self.loaded_id = None;
        self.pending_id = None;
        self.bar.set_track(None);
        self.bar.set_paused(true);
        self.bar.set_loading(false);
        self.mark_playing(None);
        self.integrations.track_changed(None);
    }

    fn mark_playing(&mut self, id: Option<&str>) {
        for e in &mut self.nav {
            if let Some(v) = e.view.as_mut() {
                v.set_playing(id);
            }
        }
    }

    fn queue_changed(&mut self) {
        self.panel.set_queue(&self.queue);
    }

    fn maybe_more_radio(&mut self, sender: &ComponentSender<Self>) {
        if self.radio_loading || self.queue.remaining() > 3 {
            return;
        }
        let (Some(src), Some(c)) = (self.queue.radio.clone(), self.ytm.clone()) else { return };
        self.radio_loading = true;
        sender.oneshot_command(async move { CmdOut::RadioMore(c.queue_more(&src.token, &src.playlist_id).await.map_err(err)) });
    }

    /// Counts a play in YouTube Music history once playback really reaches 30 s.
    fn maybe_send_history(&mut self) {
        if !self.settings.send_history || self.position < 30.0 {
            return;
        }
        let Some(id) = self.loaded_id.clone() else { return };
        if self.history_sent_for.as_deref() == Some(id.as_str()) {
            return;
        }
        self.history_sent_for = Some(id.clone());
        let Some(c) = self.ytm.clone() else { return };
        relm4::spawn(async move {
            if let Err(e) = c.add_history(&id).await {
                eprintln!("sonora: history: {e:#}");
            }
        });
    }

    fn toggle_panel(&mut self, which: Panel, sender: &ComponentSender<Self>) {
        let show = if self.panel.shown == Some(which) { None } else { Some(which) };
        self.panel.show(show);
        self.bar.queue.set_active(show == Some(Panel::Queue));
        self.bar.lyrics.set_active(show == Some(Panel::Lyrics));
        // Fine-grained lyric timing only while the lyrics panel is visible.
        if let Some(t) = self.lyrics_timer.take() {
            t.remove();
        }
        if show == Some(Panel::Lyrics) {
            self.load_lyrics(sender);
            let s = self.input.clone();
            self.lyrics_timer = Some(glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
                s.emit(AppMsg::LyricsTick);
                glib::ControlFlow::Continue
            }));
        }
    }

    fn load_lyrics(&mut self, sender: &ComponentSender<Self>) {
        let Some(t) = self.queue.current().cloned() else { return };
        if self.lyrics_for.as_deref() == Some(t.video_id.as_str()) {
            return;
        }
        let Some(c) = self.ytm.clone() else { return };
        self.lyrics_for = Some(t.video_id.clone());
        self.panel.set_lyrics_loading();
        let http = self.http.clone();
        sender.oneshot_command(async move {
            let result = lyrics::fetch(&http, &c, &t).await;
            CmdOut::Lyrics { id: t.video_id, result }
        });
    }

    fn restore_session(&mut self) {
        let Some(s) = Session::load() else { return };
        if s.queue.is_empty() {
            return;
        }
        self.queue = s.queue;
        self.integrations.track_changed(self.queue.current());
        self.integrations.shuffle_repeat_changed(self.queue.shuffle(), self.queue.repeat);
        self.bar.set_shuffle(self.queue.shuffle());
        self.bar.set_repeat(self.queue.repeat);
        self.bar.set_track(self.queue.current());
        self.bar.set_paused(true);
        self.resume_at = (s.position > 5.0).then_some(s.position);
        if let Some(at) = self.resume_at {
            self.bar.set_position(at);
        }
        self.queue_changed();
    }

    fn next_download(&mut self, sender: &ComponentSender<Self>) {
        if self.downloading {
            return;
        }
        let Some(track) = self.download_queue.pop_front() else { return };
        self.downloading = true;
        let (yt, cookies, dir) = (self.resolver.yt_dlp().clone(), self.resolver.cookies_arg(), self.settings.download_dir.clone());
        sender.spawn_oneshot_command(move || {
            let result = downloads::download(&yt, &cookies, &dir, &track).map_err(err);
            CmdOut::Downloaded { track, result }
        });
    }

    // ---------------- ui helpers ----------------

    fn toast(&mut self, text: &str) {
        self.toast_label.set_label(text);
        self.toast.set_reveal_child(true);
        if let Some(t) = self.toast_timer.take() {
            t.remove();
        }
        let r = self.toast.clone();
        self.toast_timer = Some(glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || r.set_reveal_child(false)));
    }

    fn is_dark(&self) -> bool {
        match self.settings.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => gtk::Settings::default().is_some_and(|s| s.gtk_interface_color_scheme() == gtk::InterfaceColorScheme::Dark),
        }
    }

    fn apply_theme(&self) {
        let scheme = match self.settings.theme {
            Theme::Dark => gtk::InterfaceColorScheme::Dark,
            Theme::Light => gtk::InterfaceColorScheme::Light,
            Theme::System => gtk::InterfaceColorScheme::Default,
        };
        self.css.set_prefers_color_scheme(scheme);
        if let Some(s) = gtk::Settings::default() {
            s.set_gtk_interface_color_scheme(scheme);
        }
    }

    fn apply_eq(&self) {
        if let Some(p) = &self.player {
            if let Err(e) = p.set_audio_filter(&eq::filter(&self.settings.eq_preset)) {
                eprintln!("sonora: equalizer: {e:#}");
            }
        }
    }

    fn change_setting(&mut self, change: SettingChange, sender: &ComponentSender<Self>) {
        let s = &mut self.settings;
        match change {
            SettingChange::CloseToBackground(v) => s.close_to_background = v,
            SettingChange::Notifications(v) => s.notifications = v,
            SettingChange::Tray(v) => s.tray = v,
            SettingChange::Discord(v) => s.discord = v,
            SettingChange::DiscordAppId(v) => s.discord_app_id = v.trim().to_string(),
            SettingChange::Eq(v) => {
                s.eq_preset = v;
                self.apply_eq();
            }
            SettingChange::DownloadDir(p) => self.settings.download_dir = p,
            SettingChange::SendHistory(v) => s.send_history = v,
            SettingChange::LowMemory(v) => {
                s.low_memory = v;
                self.toast("Takes effect after restarting Sonora");
            }
            SettingChange::ClearCache => {
                let _ = std::fs::remove_dir_all(crate::settings::cache_dir().join("images"));
                self.toast("Image cache cleared");
            }
        }
        self.settings.save();
        self.integrations.apply_settings(&self.settings);
        let _ = sender;
    }

    fn quit(&mut self) {
        Session { queue: self.queue.clone(), position: self.position }.save();
        self.settings.save();
        if let Some(p) = &self.player {
            let _ = p.stop();
        }
        self.hold = None;
        relm4::main_application().quit();
    }
}

fn install_shortcuts(window: &gtk::ApplicationWindow, input: &Sender<AppMsg>, search: &gtk::SearchEntry) {
    let c = gtk::ShortcutController::new();
    c.set_scope(gtk::ShortcutScope::Managed);
    let add = |trigger: &str, f: Box<dyn Fn() -> Option<AppMsg>>| {
        let s = input.clone();
        c.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::CallbackAction::new(move |_, _| match f() {
                Some(m) => {
                    s.emit(m);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Stop,
            })),
        ));
    };
    add("space", Box::new(|| Some(AppMsg::TogglePlay)));
    add("<Control>Right", Box::new(|| Some(AppMsg::Next)));
    add("<Control>Left", Box::new(|| Some(AppMsg::Prev)));
    add("<Shift>Right", Box::new(|| Some(AppMsg::SeekRelative(10.0))));
    add("<Shift>Left", Box::new(|| Some(AppMsg::SeekRelative(-10.0))));
    add("<Control>l", Box::new(|| Some(AppMsg::RateCurrent)));
    add("<Alt>Left", Box::new(|| Some(AppMsg::Back)));
    add("<Control>q", Box::new(|| Some(AppMsg::Quit)));
    add("<Control>comma", Box::new(|| Some(AppMsg::Root(Root::Settings))));
    add("m", Box::new(|| Some(AppMsg::ToggleMute)));
    add("<Control>Up", Box::new(|| Some(AppMsg::VolumeStep(5.0))));
    add("<Control>Down", Box::new(|| Some(AppMsg::VolumeStep(-5.0))));
    {
        let search = search.clone();
        add("<Control>f", Box::new(move || {
            search.grab_focus();
            None
        }));
    }
    window.add_controller(c);
}
