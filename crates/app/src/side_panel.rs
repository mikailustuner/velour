//! Right-side panel: play queue (reorderable) or lyrics (synced when available).

use crate::icon_names as icons;
use crate::images;
use crate::lyrics::{Lyrics, LyricsResult, active_line};
use crate::msg::AppMsg;
use crate::queue::PlayQueue;
use crate::widgets::icon_button;
use gtk::prelude::*;
use gtk::{gdk, glib};
use relm4::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Queue,
    Lyrics,
}

pub struct SidePanel {
    pub root: gtk::Revealer,
    stack: gtk::Stack,
    queue_list: gtk::ListBox,
    queue_rows: Vec<gtk::ListBoxRow>,
    lyrics_scroll: gtk::ScrolledWindow,
    lyrics_box: gtk::Box,
    lyrics_source: gtk::Label,
    synced: Vec<(f64, gtk::Label)>,
    active: Option<usize>,
    sender: Sender<AppMsg>,
    pub shown: Option<Panel>,
}

impl SidePanel {
    pub fn new(sender: &Sender<AppMsg>) -> Self {
        let root = gtk::Revealer::new();
        root.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        root.set_transition_duration(200);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("side-panel");
        frame.set_width_request(320);
        root.set_child(Some(&frame));

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);

        // queue
        let q = gtk::Box::new(gtk::Orientation::Vertical, 8);
        q.append(&panel_title("Up next"));
        let queue_list = gtk::ListBox::new();
        queue_list.add_css_class("tracks");
        queue_list.set_selection_mode(gtk::SelectionMode::None);
        {
            let s = sender.clone();
            queue_list.connect_row_activated(move |_, row| s.emit(AppMsg::JumpTo(row.index() as usize)));
        }
        let qs = gtk::ScrolledWindow::new();
        qs.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        qs.set_vexpand(true);
        qs.set_child(Some(&queue_list));
        q.append(&qs);
        stack.add_named(&q, Some("queue"));

        // lyrics
        let l = gtk::Box::new(gtk::Orientation::Vertical, 8);
        l.append(&panel_title("Lyrics"));
        let lyrics_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        lyrics_box.add_css_class("lyrics");
        let lyrics_scroll = gtk::ScrolledWindow::new();
        lyrics_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        lyrics_scroll.set_vexpand(true);
        lyrics_scroll.set_child(Some(&lyrics_box));
        let lyrics_source = gtk::Label::new(None);
        lyrics_source.add_css_class("dim");
        lyrics_source.add_css_class("caption");
        lyrics_source.set_xalign(0.0);
        l.append(&lyrics_scroll);
        l.append(&lyrics_source);
        stack.add_named(&l, Some("lyrics"));

        frame.append(&stack);
        Self {
            root,
            stack,
            queue_list,
            queue_rows: vec![],
            lyrics_scroll,
            lyrics_box,
            lyrics_source,
            synced: vec![],
            active: None,
            sender: sender.clone(),
            shown: None,
        }
    }

    pub fn show(&mut self, panel: Option<Panel>) {
        self.shown = panel;
        match panel {
            Some(Panel::Queue) => self.stack.set_visible_child_name("queue"),
            Some(Panel::Lyrics) => self.stack.set_visible_child_name("lyrics"),
            None => {}
        }
        self.root.set_reveal_child(panel.is_some());
    }

    /// Rebuilds the queue list.
    pub fn set_queue(&mut self, q: &PlayQueue) {
        while let Some(c) = self.queue_list.first_child() {
            self.queue_list.remove(&c);
        }
        self.queue_rows.clear();
        for t in q.tracks() {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("track-row");
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let (pic, img) = crate::widgets::small_cover(38, "thumb");
            images::load(&img, t.thumbnail.as_deref(), 38);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
            text.set_hexpand(true);
            let title = gtk::Label::new(Some(&t.title));
            title.add_css_class("row-title");
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let artist = gtk::Label::new(Some(&t.artist_line()));
            artist.add_css_class("dim");
            artist.add_css_class("caption");
            artist.set_xalign(0.0);
            artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&title);
            text.append(&artist);
            let remove = icon_button(icons::CLOSE_SYMBOLIC, "Remove from queue");
            remove.add_css_class("hover-only");
            remove.set_valign(gtk::Align::Center);
            {
                let (s, row) = (self.sender.clone(), row.clone());
                remove.connect_clicked(move |_| s.emit(AppMsg::RemoveFromQueue(row.index() as usize)));
            }
            b.append(&pic);
            b.append(&text);
            b.append(&remove);
            row.set_child(Some(&b));
            if !t.playable {
                row.add_css_class("unavailable");
                row.set_activatable(false);
            }
            self.add_dnd(&row);
            self.queue_list.append(&row);
            self.queue_rows.push(row);
        }
        self.set_queue_position(q.position());
    }

    /// Drag a row onto another to move it there.
    fn add_dnd(&self, row: &gtk::ListBoxRow) {
        let src = gtk::DragSource::new();
        src.set_actions(gdk::DragAction::MOVE);
        {
            let row = row.clone();
            src.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&(row.index() as u32).to_value())));
        }
        row.add_controller(src);
        let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
        {
            let (row, s) = (row.clone(), self.sender.clone());
            drop.connect_drop(move |_, value, _, _| {
                let Ok(from) = value.get::<u32>() else { return false };
                s.emit(AppMsg::MoveInQueue(from as usize, row.index() as usize));
                true
            });
        }
        row.add_controller(drop);
    }

    pub fn set_queue_position(&self, pos: usize) {
        for (i, r) in self.queue_rows.iter().enumerate() {
            if i == pos { r.add_css_class("playing") } else { r.remove_css_class("playing") }
            if i < pos { r.add_css_class("played") } else { r.remove_css_class("played") }
        }
    }

    pub fn set_lyrics_loading(&mut self) {
        self.clear_lyrics();
        let s = gtk::Spinner::new();
        s.set_spinning(true);
        s.set_halign(gtk::Align::Start);
        self.lyrics_box.append(&s);
    }

    pub fn set_lyrics(&mut self, r: &LyricsResult) {
        self.clear_lyrics();
        self.lyrics_source.set_label(&r.source.as_deref().map(|s| format!("Source: {}", s.trim_start_matches("Source: "))).unwrap_or_default());
        match &r.lyrics {
            Lyrics::Synced(lines) => {
                for (t, text) in lines {
                    let l = gtk::Label::new(Some(if text.is_empty() { "♪" } else { text }));
                    l.add_css_class("lyric-line");
                    l.set_xalign(0.0);
                    l.set_wrap(true);
                    // Clicking a line jumps there.
                    let click = gtk::GestureClick::new();
                    let (s, t) = (self.sender.clone(), *t);
                    click.connect_released(move |_, _, _, _| s.emit(AppMsg::Seek(t)));
                    l.add_controller(click);
                    l.set_tooltip_text(Some("Jump here"));
                    self.lyrics_box.append(&l);
                    self.synced.push((t, l));
                }
            }
            Lyrics::Plain(text) => {
                let l = gtk::Label::new(Some(text));
                l.add_css_class("lyric-plain");
                l.set_xalign(0.0);
                l.set_wrap(true);
                l.set_selectable(true);
                self.lyrics_box.append(&l);
            }
            Lyrics::None => {
                let l = gtk::Label::new(Some("No lyrics for this song."));
                l.add_css_class("dim");
                l.set_xalign(0.0);
                self.lyrics_box.append(&l);
            }
        }
    }

    fn clear_lyrics(&mut self) {
        while let Some(c) = self.lyrics_box.first_child() {
            self.lyrics_box.remove(&c);
        }
        self.synced.clear();
        self.active = None;
        self.lyrics_source.set_label("");
        self.lyrics_scroll.vadjustment().set_value(0.0);
    }

    /// Highlights the current synced line and keeps it in view.
    pub fn set_position(&mut self, pos: f64) {
        if self.synced.is_empty() || self.shown != Some(Panel::Lyrics) {
            return;
        }
        let stamps: Vec<(f64, String)> = self.synced.iter().map(|(t, _)| (*t, String::new())).collect();
        let now = active_line(&stamps, pos);
        if now == self.active {
            return;
        }
        if let Some(old) = self.active.and_then(|i| self.synced.get(i)) {
            old.1.remove_css_class("active");
        }
        self.active = now;
        if let Some((_, label)) = now.and_then(|i| self.synced.get(i)) {
            label.add_css_class("active");
            let (label, scroll) = (label.clone(), self.lyrics_scroll.clone());
            glib::idle_add_local_once(move || {
                if let Some(p) = label.compute_point(&scroll.child().unwrap(), &gtk::graphene::Point::new(0.0, 0.0)) {
                    let adj = scroll.vadjustment();
                    let target = (p.y() as f64 - adj.page_size() / 3.0).clamp(0.0, (adj.upper() - adj.page_size()).max(0.0));
                    adj.set_value(target);
                }
            });
        }
    }
}

fn panel_title(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .accessible_role(gtk::AccessibleRole::Heading)
        .css_classes(["section-title"])
        .xalign(0.0)
        .build()
}
