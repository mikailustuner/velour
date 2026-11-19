//! Desktop integrations fed by playback state: MPRIS, notifications, tray, Discord.
//! Each one is optional and fails quietly (logged once) so a missing service never breaks playback.

use crate::msg::AppMsg;
use crate::queue::Repeat;
use crate::settings::Settings;
use gtk::gio;
use gtk::glib;
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};
use relm4::Sender;
use sonora_ytm::Track;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

/// Last state sent to MPRIS, replayed when the bus connection becomes ready.
#[derive(Default)]
struct MprisState {
    meta: Option<Metadata>,
    playing: bool,
    shuffle: bool,
    loop_status: Option<LoopStatus>,
}

pub struct Integrations {
    sender: Sender<AppMsg>,
    mpris: Rc<RefCell<Option<Rc<Player>>>>,
    mpris_state: Rc<RefCell<MprisState>>,
    notifications: bool,
    notify_id: Rc<RefCell<u32>>,
    tray: Option<ksni::Handle<Tray>>,
    tray_wanted: bool,
    discord: Option<mpsc::Sender<Option<(String, String, i64)>>>,
    discord_key: String,
    current: Option<Track>,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    started_at: i64,
}

impl Integrations {
    pub fn new(sender: &Sender<AppMsg>) -> Self {
        let me = Self {
            sender: sender.clone(),
            mpris: Rc::new(RefCell::new(None)),
            mpris_state: Rc::default(),
            notifications: false,
            notify_id: Rc::new(RefCell::new(0)),
            tray: None,
            tray_wanted: false,
            discord: None,
            discord_key: String::new(),
            current: None,
            playing: false,
            shuffle: false,
            repeat: Repeat::Off,
            started_at: 0,
        };
        me.start_mpris();
        me
    }

    // ---------------- MPRIS ----------------

    fn start_mpris(&self) {
        let (slot, s, state) = (self.mpris.clone(), self.sender.clone(), self.mpris_state.clone());
        glib::spawn_future_local(async move {
            let player = match Player::builder("sonora")
                .identity("Sonora")
                .desktop_entry("dev.sonora.Sonora")
                .can_play(true)
                .can_pause(true)
                .can_go_next(true)
                .can_go_previous(true)
                .can_seek(true)
                .can_control(true)
                .can_raise(true)
                .can_quit(true)
                .build()
                .await
            {
                Ok(p) => Rc::new(p),
                Err(e) => {
                    eprintln!("sonora: MPRIS unavailable: {e}");
                    return;
                }
            };
            let wire = |f: fn() -> AppMsg| {
                let s = s.clone();
                move |_: &Player| s.emit(f())
            };
            player.connect_play_pause(wire(|| AppMsg::TogglePlay));
            player.connect_next(wire(|| AppMsg::Next));
            player.connect_previous(wire(|| AppMsg::Prev));
            player.connect_raise(wire(|| AppMsg::ShowWindow));
            player.connect_quit(wire(|| AppMsg::Quit));
            {
                // Play/Pause are idempotent in MPRIS; only toggle when the state differs.
                let s2 = s.clone();
                player.connect_play(move |p| {
                    if p.playback_status() != PlaybackStatus::Playing {
                        s2.emit(AppMsg::TogglePlay);
                    }
                });
                let s2 = s.clone();
                player.connect_pause(move |p| {
                    if p.playback_status() == PlaybackStatus::Playing {
                        s2.emit(AppMsg::TogglePlay);
                    }
                });
                let s2 = s.clone();
                player.connect_stop(move |p| {
                    if p.playback_status() == PlaybackStatus::Playing {
                        s2.emit(AppMsg::TogglePlay);
                    }
                });
                let s2 = s.clone();
                player.connect_seek(move |_, offset| s2.emit(AppMsg::SeekRelative(offset.as_micros() as f64 / 1e6)));
                let s2 = s.clone();
                player.connect_set_position(move |_, _, pos| s2.emit(AppMsg::Seek(pos.as_micros() as f64 / 1e6)));
                let s2 = s.clone();
                player.connect_set_volume(move |_, v| s2.emit(AppMsg::SetVolume((v * 100.0).clamp(0.0, 100.0))));
                let s2 = s.clone();
                player.connect_set_shuffle(move |p, on| {
                    if p.shuffle() != on {
                        s2.emit(AppMsg::ToggleShuffle);
                    }
                });
                let s2 = s.clone();
                player.connect_set_loop_status(move |p, want| {
                    // Off → All(Playlist) → One(Track) → Off is the app's cycle order.
                    let order = [LoopStatus::None, LoopStatus::Playlist, LoopStatus::Track];
                    let from = order.iter().position(|l| *l == p.loop_status()).unwrap_or(0);
                    let to = order.iter().position(|l| *l == want).unwrap_or(0);
                    for _ in 0..(to + 3 - from) % 3 {
                        s2.emit(AppMsg::CycleRepeat);
                    }
                });
            }
            glib::spawn_future_local(player.run());
            // Replay whatever happened before the bus was ready (restored session).
            let (meta, playing, shuffle, loop_status) = {
                let st = state.borrow();
                (st.meta.clone(), st.playing, st.shuffle, st.loop_status)
            };
            if let Some(m) = meta {
                let _ = player.set_metadata(m).await;
            }
            let _ = player
                .set_playback_status(if playing { PlaybackStatus::Playing } else { PlaybackStatus::Paused })
                .await;
            let _ = player.set_shuffle(shuffle).await;
            if let Some(l) = loop_status {
                let _ = player.set_loop_status(l).await;
            }
            *slot.borrow_mut() = Some(player);
        });
    }

    fn with_mpris<F, Fut>(&self, f: F)
    where
        F: FnOnce(Rc<Player>) -> Fut + 'static,
        Fut: std::future::Future<Output = mpris_server::zbus::Result<()>> + 'static,
    {
        let Some(p) = self.mpris.borrow().clone() else { return };
        glib::spawn_future_local(async move {
            if let Err(e) = f(p).await {
                eprintln!("sonora: MPRIS update: {e}");
            }
        });
    }

    // ---------------- events from the app ----------------

    pub fn track_changed(&mut self, track: Option<&Track>) {
        self.current = track.cloned();
        self.started_at = glib::real_time() / 1_000_000;
        let meta = track.map(metadata).unwrap_or_default();
        self.mpris_state.borrow_mut().meta = Some(meta.clone());
        self.with_mpris(move |p| async move {
            p.set_position(Time::ZERO);
            p.set_metadata(meta).await
        });
        if let Some(t) = track
            && self.notifications
        {
            self.notify(t);
        }
        self.update_tray();
        self.update_discord();
    }

    /// Lists without a duration column (artist top songs) only learn it from mpv.
    pub fn duration_known(&mut self, secs: f64) {
        let Some(t) = self.current.as_mut() else { return };
        if t.duration.is_some() || secs <= 0.0 {
            return;
        }
        t.duration = Some(secs as u32);
        let meta = metadata(t);
        self.mpris_state.borrow_mut().meta = Some(meta.clone());
        self.with_mpris(move |p| async move { p.set_metadata(meta).await });
    }

    pub fn playback_changed(&mut self, playing: bool) {
        if self.playing == playing {
            return;
        }
        self.playing = playing;
        self.mpris_state.borrow_mut().playing = playing;
        let status = if playing { PlaybackStatus::Playing } else { PlaybackStatus::Paused };
        self.with_mpris(move |p| async move { p.set_playback_status(status).await });
        self.update_tray();
        self.update_discord();
    }

    pub fn position_changed(&mut self, pos: f64) {
        // MPRIS clients read Position on demand; no signal needed.
        if let Some(p) = self.mpris.borrow().as_ref() {
            p.set_position(Time::from_micros((pos * 1e6) as i64));
        }
    }

    pub fn seeked(&mut self, pos: f64) {
        self.started_at = glib::real_time() / 1_000_000 - pos as i64;
        self.with_mpris(move |p| async move {
            let t = Time::from_micros((pos * 1e6) as i64);
            p.set_position(t);
            p.seeked(t).await
        });
        self.update_discord();
    }

    pub fn volume_changed(&mut self, volume: f64) {
        self.with_mpris(move |p| async move { p.set_volume(volume / 100.0).await });
    }

    pub fn shuffle_repeat_changed(&mut self, shuffle: bool, repeat: Repeat) {
        self.shuffle = shuffle;
        self.repeat = repeat;
        let loop_status = match repeat {
            Repeat::Off => LoopStatus::None,
            Repeat::All => LoopStatus::Playlist,
            Repeat::One => LoopStatus::Track,
        };
        {
            let mut st = self.mpris_state.borrow_mut();
            st.shuffle = shuffle;
            st.loop_status = Some(loop_status);
        }
        self.with_mpris(move |p| async move {
            p.set_shuffle(shuffle).await?;
            p.set_loop_status(loop_status).await
        });
    }

    pub fn apply_settings(&mut self, s: &Settings) {
        self.notifications = s.notifications;
        if s.tray != self.tray_wanted {
            self.tray_wanted = s.tray;
            if s.tray { self.start_tray() } else { self.stop_tray() }
        }
        let key = if s.discord { s.discord_app_id.clone() } else { String::new() };
        if key != self.discord_key {
            self.discord_key = key.clone();
            self.discord = None; // dropping the sender ends the old worker
            if !key.is_empty() {
                self.discord = Some(discord_worker(key));
                self.update_discord();
            }
        }
    }

    // ---------------- notifications (org.freedesktop.Notifications) ----------------

    fn notify(&self, t: &Track) {
        let (title, body) = (t.title.clone(), t.artist_line());
        let id_cell = self.notify_id.clone();
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return };
            let replaces = *id_cell.borrow();
            let hints = glib::VariantDict::new(None);
            hints.insert("desktop-entry", "dev.sonora.Sonora");
            hints.insert("transient", true);
            let args = glib::Variant::tuple_from_iter([
                "Sonora".to_variant(),
                replaces.to_variant(),
                "audio-x-generic".to_variant(),
                title.to_variant(),
                body.to_variant(),
                Vec::<String>::new().to_variant(),
                hints.end(),
                4000i32.to_variant(),
            ]);
            match bus
                .call_future(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    "org.freedesktop.Notifications",
                    "Notify",
                    Some(&args),
                    Some(glib::VariantTy::new("(u)").unwrap()),
                    gio::DBusCallFlags::NONE,
                    3000,
                )
                .await
            {
                Ok(v) => {
                    if let Some((id,)) = v.get::<(u32,)>() {
                        *id_cell.borrow_mut() = id;
                    }
                }
                Err(e) => eprintln!("sonora: notification failed (is a notification daemon running?): {e}"),
            }
        });
    }

    // ---------------- tray (StatusNotifierItem) ----------------

    fn tray_state(&self) -> Tray {
        Tray {
            sender: self.sender.clone(),
            title: self
                .current
                .as_ref()
                .map(|t| format!("{} — {}", t.title, t.artist_line()))
                .unwrap_or_else(|| "Sonora".into()),
            playing: self.playing,
        }
    }

    fn start_tray(&mut self) {
        let tray = self.tray_state();
        let s = self.sender.clone();
        glib::spawn_future_local(async move {
            use ksni::TrayMethods;
            match tray.spawn().await {
                Ok(h) => s.emit(AppMsg::TrayReady(TrayHandle(h))),
                Err(e) => eprintln!("sonora: tray unavailable (needs a StatusNotifier host): {e}"),
            }
        });
    }

    /// Called once the tray finished registering.
    pub fn tray_ready(&mut self, h: TrayHandle) {
        if self.tray_wanted {
            self.tray = Some(h.0);
            self.update_tray();
        } else {
            h.0.shutdown();
        }
    }

    fn stop_tray(&mut self) {
        if let Some(h) = self.tray.take() {
            h.shutdown();
        }
    }

    fn update_tray(&self) {
        let Some(h) = self.tray.clone() else { return };
        let state = self.tray_state();
        glib::spawn_future_local(async move {
            h.update(|t: &mut Tray| {
                t.title = state.title;
                t.playing = state.playing;
            })
            .await;
        });
    }

    // ---------------- Discord ----------------

    fn update_discord(&self) {
        let Some(tx) = &self.discord else { return };
        let msg = match (&self.current, self.playing) {
            (Some(t), true) => Some((t.title.clone(), t.artist_line(), self.started_at)),
            _ => None,
        };
        let _ = tx.send(msg);
    }
}

fn metadata(t: &Track) -> Metadata {
    let id = format!("/dev/sonora/track/{}", t.video_id.replace('-', "_"));
    let mut b = Metadata::builder()
        .trackid(TrackId::try_from(id).unwrap_or(TrackId::NO_TRACK))
        .title(t.title.clone())
        .artist(t.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>())
        .url(format!("https://music.youtube.com/watch?v={}", t.video_id));
    if let Some(a) = &t.album {
        b = b.album(a.name.clone());
    }
    if let Some(d) = t.duration {
        b = b.length(Time::from_secs(d as i64));
    }
    if let Some(u) = &t.thumbnail {
        b = b.art_url(crate::images::url_for_size(u, 544));
    }
    b.build()
}

/// Tray handle passed through `AppMsg` (needs Debug).
pub struct TrayHandle(ksni::Handle<Tray>);

impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}

use gtk::prelude::*;

pub struct Tray {
    sender: Sender<AppMsg>,
    title: String,
    playing: bool,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "sonora".into()
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn icon_name(&self) -> String {
        "dev.sonora.Sonora".into()
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.sender.emit(AppMsg::ShowWindow);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: &str, msg: fn() -> AppMsg| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |t: &mut Self| t.sender.emit(msg())),
                ..Default::default()
            }
            .into()
        };
        vec![
            item(if self.playing { "Pause" } else { "Play" }, || AppMsg::TogglePlay),
            item("Next", || AppMsg::Next),
            item("Previous", || AppMsg::Prev),
            ksni::MenuItem::Separator,
            item("Show Sonora", || AppMsg::ShowWindow),
            item("Quit", || AppMsg::Quit),
        ]
    }
}

/// Discord IPC is blocking, so it lives on its own thread. `None` clears the status.
fn discord_worker(app_id: String) -> mpsc::Sender<Option<(String, String, i64)>> {
    use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
    let (tx, rx) = mpsc::channel::<Option<(String, String, i64)>>();
    std::thread::Builder::new()
        .name("sonora-discord".into())
        .spawn(move || {
            let mut client = DiscordIpcClient::new(&app_id);
            let mut connected = false;
            let mut warned = false;
            while let Ok(mut msg) = rx.recv() {
                // Only the latest state matters.
                while let Ok(m) = rx.try_recv() {
                    msg = m;
                }
                if !connected {
                    connected = client.connect().is_ok();
                    if !connected {
                        if !warned {
                            eprintln!("sonora: Discord not reachable (is the desktop app running?)");
                            warned = true;
                        }
                        continue;
                    }
                }
                let result = match &msg {
                    Some((title, artist, start)) => client.set_activity(
                        activity::Activity::new()
                            .activity_type(activity::ActivityType::Listening)
                            .details(title)
                            .state(artist)
                            .timestamps(activity::Timestamps::new().start(*start)),
                    ),
                    None => client.clear_activity(),
                };
                if result.is_err() {
                    connected = false;
                }
            }
            let _ = client.close();
        })
        .expect("discord thread");
    tx
}
