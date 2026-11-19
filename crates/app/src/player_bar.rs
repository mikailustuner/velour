//! Bottom now-playing bar.

use crate::icon_names as icons;
use crate::images;
use crate::msg::AppMsg;
use crate::queue::Repeat;
use crate::widgets::{fmt_duration, icon_button, set_icon_button, small_cover};
use gtk::glib;
use gtk::prelude::*;
use relm4::Sender;
use sonora_ytm::{LikeStatus, Track};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct PlayerBar {
    pub root: gtk::CenterBox,
    cover: gtk::Image,
    title: gtk::Label,
    artist: gtk::Label,
    play: gtk::Button,
    spinner: gtk::Spinner,
    shuffle: gtk::Button,
    repeat: gtk::Button,
    like: gtk::Button,
    pub queue: gtk::ToggleButton,
    pub lyrics: gtk::ToggleButton,
    progress: gtk::Scale,
    elapsed: gtk::Label,
    duration_label: gtk::Label,
    volume: gtk::Scale,
    center: gtk::Box,
    mute: gtk::Button,
    duration: Cell<f64>,
    /// Pending seek while the user drags (debounced).
    seek_timer: Rc<RefCell<Option<glib::SourceId>>>,
    dragging: Rc<Cell<bool>>,
    /// Per-track CSS overriding the cover glow variables.
    glow: gtk::CssProvider,
}

/// Ambient light from the cover: glow around the bar, a tint inside it, and the window
/// background glow shifted toward the same hue. Recomputed once per track.
fn set_glow(provider: &gtk::CssProvider, color: Option<(u8, u8, u8)>) {
    let css = match color {
        Some((r, g, b)) => format!(
            "window.sonora {{ --cover-glow: rgba({r},{g},{b},0.50); --cover-tint: rgba({r},{g},{b},0.24); --cover-tint-strong: rgba({r},{g},{b},0.30); --glow-a: rgba({r},{g},{b},0.08); }}"
        ),
        None => String::new(),
    };
    provider.load_from_string(&css);
}

impl PlayerBar {
    pub fn new(sender: &Sender<AppMsg>) -> Self {
        let root = gtk::CenterBox::new();
        root.add_css_class("now-playing");

        // ---- left: cover + text ----
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        // Small minimums so the window can get narrow; natural sizes stay generous.
        left.set_width_request(150);
        let (cover_box, cover) = small_cover(52, "cover-small");
        cover.update_property(&[gtk::accessible::Property::Label("Album cover")]);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        let title = gtk::Label::new(Some("Not playing"));
        title.add_css_class("track-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(22);
        let artist = gtk::Label::new(Some("Pick a song to start"));
        artist.add_css_class("track-artist");
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
        artist.set_max_width_chars(22);
        text.append(&title);
        text.append(&artist);
        let like = icon_button(icons::FAVORITE_SYMBOLIC, "Like (Ctrl+L)");
        like.set_valign(gtk::Align::Center);
        like.set_sensitive(false);
        {
            let s = sender.clone();
            like.connect_clicked(move |_| s.emit(AppMsg::RateCurrent));
        }
        left.append(&cover_box);
        left.append(&text);
        // Cover and title open the now-playing view.
        for w in [cover_box.upcast_ref::<gtk::Widget>(), text.upcast_ref::<gtk::Widget>()] {
            let click = gtk::GestureClick::new();
            let s = sender.clone();
            click.connect_released(move |_, _, _, _| s.emit(AppMsg::ToggleNowPlaying));
            w.add_controller(click);
            w.set_cursor_from_name(Some("pointer"));
            w.set_tooltip_text(Some("Now playing (P)"));
        }
        left.append(&like);

        // ---- center: transport + progress ----
        let center = gtk::Box::new(gtk::Orientation::Vertical, 4);
        center.set_valign(gtk::Align::Center);
        center.set_width_request(300);
        center.set_hexpand(false);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        buttons.set_halign(gtk::Align::Center);
        let shuffle = icon_button(icons::SHUFFLE_SYMBOLIC, "Shuffle: off");
        shuffle.add_css_class("dim");
        let prev = icon_button(icons::SKIP_PREVIOUS_FILL_SYMBOLIC, "Previous (Ctrl+←)");
        let play = gtk::Button::from_icon_name(icons::PLAY_ARROW_FILL_SYMBOLIC);
        play.add_css_class("play");
        play.set_tooltip_text(Some("Play (Space)"));
        play.update_property(&[gtk::accessible::Property::Label("Play")]);
        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        let next = icon_button(icons::SKIP_NEXT_FILL_SYMBOLIC, "Next (Ctrl+→)");
        let repeat = icon_button(icons::REPEAT_SYMBOLIC, "Repeat: off");
        repeat.add_css_class("dim");
        let wire = |b: &gtk::Button, f: fn() -> AppMsg| {
            let s = sender.clone();
            b.connect_clicked(move |_| s.emit(f()));
        };
        wire(&shuffle, || AppMsg::ToggleShuffle);
        wire(&prev, || AppMsg::Prev);
        wire(&play, || AppMsg::TogglePlay);
        wire(&next, || AppMsg::Next);
        wire(&repeat, || AppMsg::CycleRepeat);
        for w in [&shuffle, &prev, &play, &next, &repeat] {
            w.set_valign(gtk::Align::Center);
        }
        let play_slot = gtk::Overlay::new();
        play_slot.set_child(Some(&play));
        spinner.set_halign(gtk::Align::Center);
        spinner.set_valign(gtk::Align::Center);
        spinner.set_can_target(false);
        play_slot.add_overlay(&spinner);
        buttons.append(&shuffle);
        buttons.append(&prev);
        buttons.append(&play_slot);
        buttons.append(&next);
        buttons.append(&repeat);

        let progress_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let elapsed = gtk::Label::new(Some("0:00"));
        elapsed.add_css_class("time");
        elapsed.set_width_chars(5);
        elapsed.set_xalign(1.0);
        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        progress.add_css_class("progress");
        progress.set_draw_value(false);
        progress.set_hexpand(true);
        progress.set_sensitive(false);
        progress.update_property(&[gtk::accessible::Property::Label("Seek")]);
        let duration_label = gtk::Label::new(Some("0:00"));
        duration_label.add_css_class("time");
        duration_label.set_width_chars(5);
        duration_label.set_xalign(0.0);
        progress_row.append(&elapsed);
        progress_row.append(&progress);
        progress_row.append(&duration_label);
        center.append(&buttons);
        center.append(&progress_row);

        // ---- right: panels + volume ----
        let right = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        right.set_valign(gtk::Align::Center);
        right.set_halign(gtk::Align::End);

        let lyrics = toggle(icons::LYRICS_SYMBOLIC, "Lyrics");
        let queue = toggle(icons::QUEUE_MUSIC_SYMBOLIC, "Queue");
        {
            let s = sender.clone();
            lyrics.connect_clicked(move |_| s.emit(AppMsg::ToggleLyrics));
            let s = sender.clone();
            queue.connect_clicked(move |_| s.emit(AppMsg::ToggleQueue));
        }
        let mute = icon_button(icons::VOLUME_UP_SYMBOLIC, "Mute (M)");
        mute.add_css_class("dim");
        wire(&mute, || AppMsg::ToggleMute);
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        volume.add_css_class("volume");
        volume.set_draw_value(false);
        volume.set_width_request(100);
        volume.update_property(&[gtk::accessible::Property::Label("Volume")]);
        {
            let s = sender.clone();
            volume.connect_change_value(move |_, _, v| {
                s.emit(AppMsg::SetVolume(v.clamp(0.0, 100.0)));
                glib::Propagation::Proceed
            });
        }
        right.append(&lyrics);
        right.append(&queue);
        right.append(&mute);
        right.append(&volume);

        root.set_start_widget(Some(&left));
        root.set_center_widget(Some(&center));
        root.set_end_widget(Some(&right));

        let seek_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        let dragging = Rc::new(Cell::new(false));
        {
            // Debounce seeks: dragging emits many values; seek once it settles.
            let (s, timer, dragging, elapsed) = (sender.clone(), seek_timer.clone(), dragging.clone(), elapsed.clone());
            progress.connect_change_value(move |_, _, v| {
                dragging.set(true);
                elapsed.set_label(&fmt_duration(v.max(0.0) as u32));
                if let Some(id) = timer.borrow_mut().take() {
                    id.remove();
                }
                let (s, timer2, dragging) = (s.clone(), timer.clone(), dragging.clone());
                *timer.borrow_mut() = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                    timer2.borrow_mut().take();
                    dragging.set(false);
                    s.emit(AppMsg::Seek(v.max(0.0)));
                }));
                glib::Propagation::Proceed
            });
        }

        let glow = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(&gtk::prelude::WidgetExt::display(&root), &glow, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);

        Self {
            glow,
            center: center.clone(),
            root,
            cover,
            title,
            artist,
            play,
            spinner,
            shuffle,
            repeat,
            like,
            queue,
            lyrics,
            progress,
            elapsed,
            duration_label,
            volume,
            mute,
            duration: Cell::new(0.0),
            seek_timer,
            dragging,
        }
    }

    pub fn set_track(&self, t: Option<&Track>) {
        match t {
            Some(t) => {
                self.title.set_label(&t.title);
                self.title.set_tooltip_text(Some(&t.title));
                let a = t.artist_line();
                self.artist.set_label(&a);
                self.artist.set_tooltip_text(Some(&a));
                let glow = self.glow.clone();
                images::load_then(&self.cover, t.thumbnail.as_deref(), 52, move |tex| {
                    set_glow(&glow, images::accent_color(tex));
                });
                self.set_like(t.like);
                self.like.set_sensitive(true);
                self.progress.set_sensitive(true);
                self.set_duration(t.duration.unwrap_or(0) as f64);
            }
            None => {
                self.title.set_label("Not playing");
                self.artist.set_label("Pick a song to start");
                images::load(&self.cover, None, 52);
                set_glow(&self.glow, None);
                self.like.set_sensitive(false);
                self.progress.set_sensitive(false);
                self.set_duration(0.0);
            }
        }
        self.set_position(0.0);
    }

    /// Narrow windows: hide the volume slider and tighten the columns.
    /// Narrow windows: hide the volume slider. Minimum widths stay small at all times
    /// (a window can't shrink below its minimum, so they must not depend on its width).
    pub fn set_compact(&self, compact: bool) {
        self.volume.set_visible(!compact);
        self.center.set_size_request(if compact { 300 } else { 440 }, -1);
    }

    pub fn set_like(&self, like: Option<LikeStatus>) {
        if like == Some(LikeStatus::Like) {
            set_icon_button(&self.like, icons::FAVORITE_FILL_SYMBOLIC, "Remove from Liked (Ctrl+L)");
            self.like.add_css_class("liked");
        } else {
            set_icon_button(&self.like, icons::FAVORITE_SYMBOLIC, "Like (Ctrl+L)");
            self.like.remove_css_class("liked");
        }
    }

    pub fn set_loading(&self, loading: bool) {
        self.spinner.set_visible(loading);
        self.spinner.set_spinning(loading);
        self.play.set_opacity(if loading { 0.4 } else { 1.0 });
    }

    pub fn set_paused(&self, paused: bool) {
        let (icon, label) = if paused { (icons::PLAY_ARROW_FILL_SYMBOLIC, "Play") } else { (icons::PAUSE_FILL_SYMBOLIC, "Pause") };
        self.play.set_icon_name(icon);
        self.play.set_tooltip_text(Some(&format!("{label} (Space)")));
        self.play.update_property(&[gtk::accessible::Property::Label(label)]);
    }

    pub fn set_duration(&self, d: f64) {
        self.duration.set(d);
        self.progress.set_range(0.0, d.max(1.0));
        self.duration_label.set_label(&fmt_duration(d as u32));
    }

    pub fn set_position(&self, p: f64) {
        if self.dragging.get() || self.seek_timer.borrow().is_some() {
            return;
        }
        // Unknown duration (restored session): keep the bar empty rather than full.
        self.progress.set_value(if self.duration.get() > 0.0 { p } else { 0.0 });
        self.elapsed.set_label(&fmt_duration(p.max(0.0) as u32));
    }

    pub fn set_volume(&self, v: f64, muted: bool) {
        self.volume.set_value(v);
        let (icon, label) = if muted || v == 0.0 { (icons::VOLUME_OFF_SYMBOLIC, "Unmute (M)") } else { (icons::VOLUME_UP_SYMBOLIC, "Mute (M)") };
        set_icon_button(&self.mute, icon, label);
    }

    pub fn set_shuffle(&self, on: bool) {
        set_icon_button(&self.shuffle, icons::SHUFFLE_SYMBOLIC, if on { "Shuffle: on" } else { "Shuffle: off" });
        toggle_class(&self.shuffle, "active", on);
        toggle_class(&self.shuffle, "dim", !on);
    }

    pub fn set_repeat(&self, r: Repeat) {
        let (icon, label) = match r {
            Repeat::Off => (icons::REPEAT_SYMBOLIC, "Repeat: off"),
            Repeat::All => (icons::REPEAT_SYMBOLIC, "Repeat: all"),
            Repeat::One => (icons::REPEAT_ONE_SYMBOLIC, "Repeat: one"),
        };
        set_icon_button(&self.repeat, icon, label);
        toggle_class(&self.repeat, "active", r != Repeat::Off);
        toggle_class(&self.repeat, "dim", r == Repeat::Off);
    }
}

fn toggle(icon: &str, label: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    b.set_icon_name(icon);
    b.add_css_class("icon");
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

fn toggle_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on { w.add_css_class(class) } else { w.remove_css_class(class) }
}
