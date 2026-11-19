//! Renders a `velour_ytm::Page` (header, track lists, carousels, grids) into GTK widgets.

use crate::icon_names as icons;
use crate::images;
use crate::msg::AppMsg;
use crate::widgets::{fmt_duration, icon_button};
use gtk::glib;
use gtk::prelude::*;
use relm4::Sender;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use velour_ytm::{Header, Item, Layout, LikeStatus, Page, PageType, Section, Target, Track};

/// Shared state a page needs to build its rows and menus.
#[derive(Clone)]
pub struct Ctx {
    pub sender: Sender<AppMsg>,
    /// Own playlist shown on this page (enables "Remove from playlist").
    pub editable_playlist: Option<String>,
    /// The user's own playlists for "Add to playlist": (id, title).
    pub own_playlists: Rc<Vec<(String, String)>>,
    pub downloaded: Rc<HashSet<String>>,
    /// Offline downloads enabled in Settings (opt-in).
    pub downloads_enabled: bool,
    pub playing: Option<String>,
}

/// A rendered page that can grow (continuations) and mark the playing track.
pub struct PageView {
    pub root: gtk::ScrolledWindow,
    content: gtk::Box,
    /// Last track list on the page; continuations append here.
    last_list: Option<TrackList>,
    rows: Rc<RefCell<Vec<(String, gtk::ListBoxRow)>>>,
    ctx: Ctx,
}

#[derive(Clone)]
struct TrackList {
    list: gtk::ListBox,
    tracks: Rc<RefCell<Vec<Track>>>,
    numbered: bool,
}

impl PageView {
    /// `top` is an optional widget shown above everything (e.g. library tabs).
    pub fn new(page: &Page, title: &str, top: Option<gtk::Widget>, ctx: Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 32);
        content.add_css_class("page");

        let root = gtk::ScrolledWindow::new();
        root.set_hscrollbar_policy(gtk::PolicyType::Never);
        root.set_vexpand(true);
        root.set_child(Some(&content));
        {
            let sender = ctx.sender.clone();
            root.connect_edge_reached(move |_, pos| {
                if pos == gtk::PositionType::Bottom {
                    sender.emit(AppMsg::LoadMore);
                }
            });
        }

        let mut view = Self { root, content, last_list: None, rows: Rc::new(RefCell::new(vec![])), ctx };

        match &page.header {
            Some(h) => {
                let tracks = page.tracks();
                view.content.append(&header(h, &tracks, &view.ctx));
            }
            None if !title.is_empty() => view.content.append(&large_title(title)),
            None => {}
        }
        // Extra controls (library tabs) go under the title.
        if let Some(t) = top {
            view.content.append(&t);
        }
        if page.sections.is_empty() {
            view.content.append(&empty_state(icons::MUSIC_NOTE_SYMBOLIC, "Nothing here yet."));
        }
        let numbered = page.header.as_ref().is_some_and(
            |h| matches!(&h.play, Some(Target::Watch { playlist_id: Some(p), .. }) if p.starts_with("OLAK")),
        );
        for s in &page.sections {
            view.add_section(s, numbered);
        }
        view
    }

    fn add_section(&mut self, s: &Section, numbered: bool) {
        let block = gtk::Box::new(gtk::Orientation::Vertical, 12);
        if !s.title.is_empty() {
            block.append(&section_title(&s.title, s.more.clone(), &self.ctx.sender));
        }
        match s.layout {
            Layout::List => {
                let tl = TrackList { list: gtk::ListBox::new(), tracks: Rc::new(RefCell::new(vec![])), numbered };
                tl.list.add_css_class("tracks");
                tl.list.set_selection_mode(gtk::SelectionMode::None);
                tl.list.set_activate_on_single_click(false);
                {
                    // Play in the order currently shown (after filter/sort).
                    let tracks = tl.tracks.clone();
                    let sender = self.ctx.sender.clone();
                    tl.list.connect_row_activated(move |list, row| {
                        let all = tracks.borrow();
                        let mut visible = vec![];
                        let mut start = 0;
                        let mut child = list.first_child();
                        while let Some(w) = child {
                            if let Ok(r) = w.clone().downcast::<gtk::ListBoxRow>()
                                && r.is_child_visible()
                                && let Some(t) = row_index(&r).and_then(|i| all.get(i))
                            {
                                if r == *row {
                                    start = visible.len();
                                }
                                visible.push(t.clone());
                            }
                            child = w.next_sibling();
                        }
                        if visible.get(start).is_some_and(|t| t.playable) {
                            sender.emit(AppMsg::PlayTracks { tracks: visible, start, shuffle: false });
                        }
                    });
                }
                if !numbered && s.items.len() >= 15 {
                    block.append(&list_tools(&tl));
                }
                self.append_tracks(&tl, s.items.iter().filter_map(as_track));
                block.append(&tl.list);
                self.last_list = Some(tl);
            }
            Layout::Carousel => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
                let tracks: Vec<Track> = s.items.iter().filter_map(as_track).cloned().collect();
                for item in &s.items {
                    row.append(&tile(item, &tracks, &self.ctx));
                }
                let scroller = gtk::ScrolledWindow::new();
                scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
                scroller.set_propagate_natural_height(true);
                scroller.set_child(Some(&row));
                scroller.add_css_class("carousel");
                block.append(&scroller);
            }
            Layout::Grid => {
                let grid = gtk::FlowBox::new();
                grid.set_selection_mode(gtk::SelectionMode::None);
                grid.set_column_spacing(16);
                grid.set_row_spacing(20);
                grid.set_max_children_per_line(12);
                grid.set_homogeneous(false);
                let tracks: Vec<Track> = s.items.iter().filter_map(as_track).cloned().collect();
                for item in &s.items {
                    grid.append(&tile(item, &tracks, &self.ctx));
                }
                block.append(&grid);
            }
        }
        self.content.append(&block);
    }

    fn append_tracks<'a>(&self, tl: &TrackList, tracks: impl Iterator<Item = &'a Track>) {
        for t in tracks {
            let index = tl.tracks.borrow().len();
            let row = track_row(t, index, tl.numbered, &self.ctx);
            // Original position, used by filter/sort and "play from here".
            row.set_widget_name(&index.to_string());
            if self.ctx.playing.as_deref() == Some(t.video_id.as_str()) {
                row.add_css_class("playing");
            }
            tl.list.append(&row);
            self.rows.borrow_mut().push((t.video_id.clone(), row));
            tl.tracks.borrow_mut().push(t.clone());
        }
    }

    /// Appends a continuation page (more tracks or more sections).
    pub fn append(&mut self, page: &Page) {
        for s in &page.sections {
            match (&self.last_list, s.layout, s.title.is_empty()) {
                (Some(tl), Layout::List, true) => {
                    let tl = tl.clone();
                    self.append_tracks(&tl, s.items.iter().filter_map(as_track));
                }
                _ => self.add_section(s, false),
            }
        }
    }

    pub fn set_playing(&mut self, video_id: Option<&str>) {
        self.ctx.playing = video_id.map(str::to_string);
        for (id, row) in self.rows.borrow().iter() {
            if Some(id.as_str()) == video_id {
                row.add_css_class("playing");
            } else {
                row.remove_css_class("playing");
            }
        }
    }
}

/// Asks the app to resolve `id` when the pointer rests on `w` for a moment.
fn prefetch_on_hover(w: &impl IsA<gtk::Widget>, id: Option<String>, sender: &Sender<AppMsg>) {
    let Some(id) = id else { return };
    let motion = gtk::EventControllerMotion::new();
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    {
        let (timer, sender) = (timer.clone(), sender.clone());
        motion.connect_enter(move |_, _, _| {
            let (id, sender, t2) = (id.clone(), sender.clone(), timer.clone());
            if let Some(old) = timer.borrow_mut().replace(glib::timeout_add_local_once(
                std::time::Duration::from_millis(500),
                move || {
                    t2.borrow_mut().take();
                    sender.emit(AppMsg::Prefetch(id));
                },
            )) {
                old.remove();
            }
        });
    }
    motion.connect_leave(move |_| {
        if let Some(t) = timer.borrow_mut().take() {
            t.remove();
        }
    });
    w.as_ref().add_controller(motion);
}

/// Lowercase + Turkish/Latin letter folding so "sarki" finds "Şarkı" and "istanbul" "İstanbul".
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| *c != '\u{307}')
        .map(|c| match c {
            'ı' => 'i',
            'ş' => 's',
            'ğ' => 'g',
            'ü' => 'u',
            'ö' => 'o',
            'ç' => 'c',
            'â' | 'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'í' | 'ï' => 'i',
            'ô' | 'ó' => 'o',
            'û' | 'ú' => 'u',
            c => c,
        })
        .collect()
}

fn row_index(r: &gtk::ListBoxRow) -> Option<usize> {
    r.widget_name().parse().ok()
}

/// Filter box + sort menu for long track lists.
fn list_tools(tl: &TrackList) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("list-tools");
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some("Filter songs"));
    filter.update_property(&[gtk::accessible::Property::Label("Filter songs in this list")]);
    filter.set_width_chars(24);
    let sort = gtk::DropDown::from_strings(&["List order", "Title", "Artist", "Album", "Duration"]);
    sort.update_property(&[gtk::accessible::Property::Label("Sort songs")]);
    sort.set_tooltip_text(Some("Sort songs"));
    bar.append(&filter);
    bar.append(&sort);

    let query: Rc<RefCell<String>> = Rc::default();
    {
        let (tracks, query) = (tl.tracks.clone(), query.clone());
        tl.list.set_filter_func(move |row| {
            let q = query.borrow();
            if q.is_empty() {
                return true;
            }
            let all = tracks.borrow();
            let Some(t) = row_index(row).and_then(|i| all.get(i)) else { return true };
            let hay = fold(&format!(
                "{} {} {}",
                t.title,
                t.artist_line(),
                t.album.as_ref().map(|a| a.name.as_str()).unwrap_or("")
            ));
            q.split_whitespace().all(|w| hay.contains(w))
        });
    }
    {
        let (list, query) = (tl.list.clone(), query.clone());
        filter.connect_search_changed(move |e| {
            *query.borrow_mut() = fold(&e.text());
            list.invalidate_filter();
        });
    }
    let mode = Rc::new(std::cell::Cell::new(0u32));
    {
        let (tracks, mode) = (tl.tracks.clone(), mode.clone());
        tl.list.set_sort_func(move |a, b| {
            let (ia, ib) = (row_index(a).unwrap_or(0), row_index(b).unwrap_or(0));
            let all = tracks.borrow();
            let (Some(x), Some(y)) = (all.get(ia), all.get(ib)) else { return gtk::Ordering::Equal };
            let key = |t: &Track| -> String {
                match mode.get() {
                    1 => t.title.to_lowercase(),
                    2 => t.artist_line().to_lowercase(),
                    3 => t.album.as_ref().map(|a| a.name.to_lowercase()).unwrap_or_default(),
                    _ => String::new(),
                }
            };
            let ord = match mode.get() {
                0 => ia.cmp(&ib),
                4 => x.duration.cmp(&y.duration).then(ia.cmp(&ib)),
                _ => key(x).cmp(&key(y)).then(ia.cmp(&ib)),
            };
            ord.into()
        });
    }
    {
        let list = tl.list.clone();
        sort.connect_selected_notify(move |d| {
            mode.set(d.selected());
            list.invalidate_sort();
        });
    }
    bar
}

fn as_track(i: &Item) -> Option<&Track> {
    match i {
        Item::Track(t) => Some(t),
        _ => None,
    }
}

// ---------- pieces ----------

pub fn large_title(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .accessible_role(gtk::AccessibleRole::Heading)
        .css_classes(["large-title"])
        .xalign(0.0)
        .wrap(true)
        .build()
}

fn section_title(text: &str, more: Option<Target>, sender: &Sender<AppMsg>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let l = gtk::Label::builder()
        .label(text)
        .accessible_role(gtk::AccessibleRole::Heading)
        .css_classes(["section-title"])
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    b.append(&l);
    if let Some(t) = more {
        let btn = gtk::Button::with_label("See all");
        btn.add_css_class("link-button");
        let sender = sender.clone();
        btn.connect_clicked(move |_| sender.emit(AppMsg::Open(t.clone())));
        b.append(&btn);
    }
    b
}

pub fn empty_state(icon: &str, text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.set_vexpand(true);
    b.set_valign(gtk::Align::Center);
    b.set_margin_top(48);
    let i = gtk::Image::from_icon_name(icon);
    i.set_pixel_size(48);
    i.add_css_class("dim");
    let l = gtk::Label::new(Some(text));
    l.add_css_class("empty-body");
    l.set_wrap(true);
    l.set_justify(gtk::Justification::Center);
    b.append(&i);
    b.append(&l);
    b
}

/// Placeholder with the page title and skeleton rows while a page loads.
pub fn loading_view(title: &str) -> gtk::Widget {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.add_css_class("page");
    if !title.is_empty() {
        content.append(&large_title(title));
    }
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_halign(gtk::Align::Start);
    spinner.update_property(&[gtk::accessible::Property::Label("Loading")]);
    content.append(&spinner);
    for _ in 0..8 {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let thumb = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        thumb.set_size_request(40, 40);
        thumb.add_css_class("skeleton");
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        line.set_size_request(260, 14);
        line.set_valign(gtk::Align::Center);
        line.add_css_class("skeleton");
        row.append(&thumb);
        row.append(&line);
        content.append(&row);
    }
    let s = gtk::ScrolledWindow::new();
    s.set_child(Some(&content));
    s.upcast()
}

pub fn error_view(title: &str, message: &str, sender: &Sender<AppMsg>) -> gtk::Widget {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.add_css_class("page");
    if !title.is_empty() {
        content.append(&large_title(title));
    }
    let b = empty_state(icons::MUSIC_NOTE_SYMBOLIC, &format!("Couldn’t load this page.\n{message}"));
    let retry = gtk::Button::with_label("Try again");
    retry.add_css_class("pill");
    retry.set_halign(gtk::Align::Center);
    let s = sender.clone();
    retry.connect_clicked(move |_| s.emit(AppMsg::Reload));
    b.append(&retry);
    content.append(&b);
    content.upcast()
}

fn picture(px_w: i32, px_h: i32, round: bool) -> gtk::Picture {
    let p = gtk::Picture::new();
    p.set_size_request(px_w, px_h);
    p.set_content_fit(gtk::ContentFit::Cover);
    p.set_can_shrink(true);
    p.set_overflow(gtk::Overflow::Hidden);
    p.add_css_class(if round { "cover-round" } else { "cover" });
    p.set_halign(gtk::Align::Start);
    p.set_valign(gtk::Align::Start);
    p
}

fn header(h: &Header, tracks: &[Track], ctx: &Ctx) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    b.add_css_class("page-header");
    if h.thumbnail.is_some() {
        let pic = picture(200, 200, h.round);
        images::load(&pic, h.thumbnail.as_deref(), 200);
        pic.set_alternative_text(Some(&h.title));
        b.append(&pic);
    }
    let info = gtk::Box::new(gtk::Orientation::Vertical, 6);
    info.set_valign(gtk::Align::End);
    info.set_hexpand(true);
    info.append(&large_title(&h.title));
    for line in [&h.subtitle, &h.second_subtitle] {
        if !line.is_empty() {
            let l = gtk::Label::new(Some(line));
            l.add_css_class("dim");
            l.set_xalign(0.0);
            l.set_wrap(true);
            info.append(&l);
        }
    }
    if !h.description.is_empty() {
        // First paragraph only; `set_lines` limits lines per paragraph, not in total.
        let first = h.description.split("\n").find(|p| !p.trim().is_empty()).unwrap_or("");
        let d = gtk::Label::new(Some(first));
        d.add_css_class("description");
        d.set_xalign(0.0);
        d.set_wrap(true);
        d.set_lines(3);
        d.set_ellipsize(gtk::pango::EllipsizeMode::End);
        d.set_tooltip_text(Some(&h.description));
        d.set_max_width_chars(90);
        info.append(&d);
    }

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_margin_top(8);
    let sender = &ctx.sender;
    let has_tracks = tracks.iter().any(|t| t.playable);
    if has_tracks || h.play.is_some() {
        let play = pill_button(icons::PLAY_ARROW_FILL_SYMBOLIC, "Play", true);
        let (s, t, target) = (sender.clone(), tracks.to_vec(), h.play.clone());
        play.connect_clicked(move |_| {
            if t.iter().any(|x| x.playable) {
                s.emit(AppMsg::PlayTracks { tracks: t.clone(), start: 0, shuffle: false });
            } else if let Some(target) = &target {
                s.emit(AppMsg::PlayTarget(target.clone()));
            }
        });
        prefetch_on_hover(&play, tracks.iter().find(|t| t.playable).map(|t| t.video_id.clone()), sender);
        buttons.append(&play);
    }
    if has_tracks {
        let shuffle = pill_button(icons::SHUFFLE_SYMBOLIC, "Shuffle", false);
        let (s, t) = (sender.clone(), tracks.to_vec());
        shuffle.connect_clicked(move |_| {
            let start = rand::random_range(0..t.len());
            s.emit(AppMsg::PlayTracks { tracks: t.clone(), start, shuffle: true });
        });
        buttons.append(&shuffle);
    }
    if let Some(radio) = &h.radio {
        let r = pill_button(icons::PLAYLIST_PLAY_SYMBOLIC, "Radio", false);
        let (s, radio) = (sender.clone(), radio.clone());
        r.connect_clicked(move |_| s.emit(AppMsg::PlayTarget(radio.clone())));
        buttons.append(&r);
    }
    if has_tracks && ctx.downloads_enabled {
        let dl = icon_button(icons::DOWNLOAD_SYMBOLIC, "Download all");
        let (s, t) = (sender.clone(), tracks.to_vec());
        dl.connect_clicked(move |_| s.emit(AppMsg::Download(t.iter().filter(|x| x.playable).cloned().collect())));
        buttons.append(&dl);
    }
    if let Some(pl) = &h.playlist_id {
        let rename = icon_button(icons::EDIT_SYMBOLIC, "Rename playlist");
        let (s, id, title) = (sender.clone(), pl.clone(), h.title.clone());
        rename.connect_clicked(move |b| {
            ask_text(b, "Rename playlist", &title, {
                let (s, id) = (s.clone(), id.clone());
                move |t| s.emit(AppMsg::RenamePlaylist { playlist_id: id.clone(), title: t })
            })
        });
        buttons.append(&rename);
        let del = icon_button(icons::DELETE_SYMBOLIC, "Delete playlist");
        let (s, id) = (sender.clone(), pl.clone());
        del.connect_clicked(move |b| {
            confirm(b, "Delete this playlist?", "Delete", {
                let (s, id) = (s.clone(), id.clone());
                move || s.emit(AppMsg::DeletePlaylist(id.clone()))
            })
        });
        buttons.append(&del);
    } else if let (false, Some(Target::Watch { playlist_id: Some(pl), .. })) = (h.round, &h.play) {
        // Albums and other people's playlists can be saved to the library (not artists).
        if pl != "LM" {
            let save = icon_button(icons::LIBRARY_ADD_SYMBOLIC, "Save to library");
            let (s, id) = (sender.clone(), pl.clone());
            save.connect_clicked(move |_| s.emit(AppMsg::SaveToLibrary { playlist_id: id.clone(), save: true }));
            buttons.append(&save);
        }
    }
    info.append(&buttons);
    b.append(&info);
    b
}

fn pill_button(icon: &str, label: &str, primary: bool) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(label)));
    let b = gtk::Button::new();
    b.set_child(Some(&content));
    b.add_css_class("pill");
    if primary {
        b.add_css_class("primary");
    }
    b
}

/// Album/playlist/artist tile, or a track tile inside a carousel.
fn tile(item: &Item, section_tracks: &[Track], ctx: &Ctx) -> gtk::Widget {
    let b = gtk::Button::new();
    b.add_css_class("tile");
    let v = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.set_child(Some(&v));
    let sender = ctx.sender.clone();

    let (title, subtitle) = match item {
        Item::Card(c) if c.thumbnail.is_none() => {
            // Mood/genre button.
            b.remove_css_class("tile");
            b.add_css_class("chip");
            b.set_label(&c.title);
            let t = c.target.clone();
            b.connect_clicked(move |_| sender.emit(AppMsg::Open(t.clone())));
            return b.upcast();
        }
        Item::Card(c) => {
            let (w, h) = if c.wide { (284, 160) } else { (160, 160) };
            let pic = picture(w, h, c.round);
            if c.wide {
                images::load_wide(&pic, c.thumbnail.as_deref(), h as u32);
            } else {
                images::load(&pic, c.thumbnail.as_deref(), h as u32);
            }
            v.append(&pic);
            if let Target::Watch { video_id, .. } = &c.target {
                prefetch_on_hover(&b, video_id.clone(), &ctx.sender);
            }
            let msg_target = c.target.clone();
            b.connect_clicked(move |_| {
                let msg = match &msg_target {
                    Target::Watch { .. } => AppMsg::PlayTarget(msg_target.clone()),
                    Target::Browse { .. } => AppMsg::Open(msg_target.clone()),
                };
                sender.emit(msg);
            });
            v.set_width_request(w);
            (c.title.clone(), c.subtitle.clone())
        }
        Item::Track(t) => {
            let pic = picture(160, 160, false);
            images::load(&pic, t.thumbnail.as_deref(), 160);
            let more = menu_button(t, ctx);
            more.add_css_class("tile-more");
            more.set_halign(gtk::Align::End);
            more.set_valign(gtk::Align::End);
            more.set_margin_end(6);
            more.set_margin_bottom(6);
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&pic));
            overlay.add_overlay(&more);
            v.append(&overlay);
            attach_track_menu(&b, &more);
            prefetch_on_hover(&b, Some(t.video_id.clone()), &ctx.sender);
            v.set_width_request(160);
            let start = section_tracks.iter().position(|x| x.video_id == t.video_id).unwrap_or(0);
            let tracks = section_tracks.to_vec();
            b.connect_clicked(move |_| {
                sender.emit(AppMsg::PlayTracks { tracks: tracks.clone(), start, shuffle: false })
            });
            (t.title.clone(), t.artist_line())
        }
    };
    let tl = gtk::Label::new(Some(&title));
    tl.add_css_class("tile-title");
    tl.set_xalign(0.0);
    // One line keeps every tile the same height (full text is in the tooltip).
    tl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    tl.set_max_width_chars(1);
    tl.set_hexpand(true);
    v.append(&tl);
    if !subtitle.is_empty() {
        let sl = gtk::Label::new(Some(&subtitle));
        sl.add_css_class("dim");
        sl.add_css_class("caption");
        sl.set_xalign(0.0);
        sl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        sl.set_max_width_chars(1);
        v.append(&sl);
    }
    b.set_tooltip_text(Some(&if subtitle.is_empty() { title.clone() } else { format!("{title}\n{subtitle}") }));
    b.upcast()
}

/// Artist and album names as clickable links.
fn byline_markup(t: &Track, with_album: bool) -> String {
    let link = |name: &str, id: &Option<String>| match id {
        Some(id) => {
            format!("<a href=\"browse:{}\">{}</a>", glib::markup_escape_text(id), glib::markup_escape_text(name))
        }
        None => glib::markup_escape_text(name).to_string(),
    };
    let mut parts: Vec<String> = vec![];
    let artists: Vec<String> = t.artists.iter().map(|a| link(&a.name, &a.browse_id)).collect();
    if !artists.is_empty() {
        parts.push(artists.join(", "));
    }
    if with_album && let Some(a) = &t.album {
        parts.push(link(&a.name, &a.browse_id));
    }
    if let Some(d) = &t.detail {
        parts.push(glib::markup_escape_text(d).to_string());
    }
    parts.join(" • ")
}

fn link_label(markup: &str, sender: &Sender<AppMsg>) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_markup(markup);
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let s = sender.clone();
    l.connect_activate_link(move |_, uri| {
        if let Some(id) = uri.strip_prefix("browse:") {
            let page_type = velour_ytm::PageType::from_id(id);
            s.emit(AppMsg::Open(Target::Browse { id: id.to_string(), params: None, page_type }));
        }
        glib::Propagation::Stop
    });
    l
}

fn track_row(t: &Track, index: usize, numbered: bool, ctx: &Ctx) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("track-row");
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_child(Some(&b));

    if numbered {
        let n = gtk::Label::new(Some(&(index + 1).to_string()));
        n.add_css_class("dim");
        n.add_css_class("tnum");
        n.set_width_chars(3);
        n.set_xalign(1.0);
        b.append(&n);
    } else {
        let (frame, img) = crate::widgets::small_cover(42, "thumb");
        images::load(&img, t.thumbnail.as_deref(), 42);
        b.append(&frame);
    }

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&t.title));
    title.add_css_class("row-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&title);
    let by = byline_markup(t, false);
    if !by.is_empty() {
        let l = link_label(&by, &ctx.sender);
        l.add_css_class("dim");
        l.add_css_class("caption");
        text.append(&l);
    }
    b.append(&text);

    if let Some(a) = &t.album
        && !numbered
    {
        let markup = byline_markup(&Track { artists: vec![], detail: None, ..t.clone() }, true);
        let l = link_label(&markup, &ctx.sender);
        l.add_css_class("dim");
        l.add_css_class("album-col");
        l.set_width_chars(24);
        l.set_max_width_chars(24);
        l.set_tooltip_text(Some(&a.name));
        b.append(&l);
    }

    if ctx.downloaded.contains(&t.video_id) {
        let d = gtk::Image::from_icon_name(icons::DOWNLOAD_DONE_SYMBOLIC);
        d.add_css_class("dim");
        d.set_tooltip_text(Some("Downloaded"));
        b.append(&d);
    }

    let liked = t.like == Some(LikeStatus::Like);
    let like = icon_button(
        if liked { icons::FAVORITE_FILL_SYMBOLIC } else { icons::FAVORITE_SYMBOLIC },
        if liked { "Remove from Liked" } else { "Like" },
    );
    like.add_css_class(if liked { "liked" } else { "hover-only" });
    {
        let (s, track) = (ctx.sender.clone(), t.clone());
        like.connect_clicked(move |_| {
            let status = if track.like == Some(LikeStatus::Like) { LikeStatus::Indifferent } else { LikeStatus::Like };
            s.emit(AppMsg::Rate(track.clone(), status));
        });
    }
    b.append(&like);

    let dur = gtk::Label::new(Some(&t.duration.map(fmt_duration).unwrap_or_default()));
    dur.add_css_class("dim");
    dur.add_css_class("tnum");
    dur.set_width_chars(5);
    dur.set_xalign(1.0);
    b.append(&dur);

    let more = menu_button(t, ctx);
    b.append(&more);

    if !t.playable {
        row.add_css_class("unavailable");
        row.set_activatable(false);
        row.set_tooltip_text(Some("This song isn’t available."));
    }
    attach_track_menu(&row, &more);
    if t.playable {
        prefetch_on_hover(&row, Some(t.video_id.clone()), &ctx.sender);
    }
    row
}

/// "⋯" button that opens the track menu (built lazily on first open).
fn menu_button(t: &Track, ctx: &Ctx) -> gtk::MenuButton {
    let more = gtk::MenuButton::new();
    more.set_icon_name(icons::MORE_HORIZ_SYMBOLIC);
    more.add_css_class("icon");
    more.add_css_class("hover-only");
    more.set_tooltip_text(Some("More options"));
    more.update_property(&[gtk::accessible::Property::Label("More options")]);
    let (track, ctx) = (t.clone(), ctx.clone());
    more.set_create_popup_func(move |mb| mb.set_popover(Some(&track_menu(&track, &ctx))));
    more
}

/// Right-click opens the same menu as the ⋯ button. (A separately positioned popover
/// made the rows below it stop drawing while open.)
fn attach_track_menu(w: &impl IsA<gtk::Widget>, button: &gtk::MenuButton) {
    let click = gtk::GestureClick::new();
    click.set_button(3);
    let button = button.clone();
    click.connect_pressed(move |g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        button.popup();
    });
    w.as_ref().add_controller(click);
}

fn menu_item(label: &str, icon: &str) -> gtk::Button {
    let b = gtk::Button::new();
    let c = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    c.append(&gtk::Image::from_icon_name(icon));
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    c.append(&l);
    b.set_child(Some(&c));
    b.add_css_class("menu-item");
    b
}

/// Context menu for a track. Two pages: actions and "Add to playlist".
pub fn track_menu(t: &Track, ctx: &Ctx) -> gtk::Popover {
    let pop = gtk::Popover::new();
    pop.add_css_class("menu");
    let stack = gtk::Stack::new();
    stack.set_vhomogeneous(false);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(150);
    let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let sender = ctx.sender.clone();

    let add = |b: &gtk::Box, label: &str, icon: &str, msg: Box<dyn Fn() -> AppMsg>| {
        let item = menu_item(label, icon);
        let (s, p) = (sender.clone(), pop.clone());
        item.connect_clicked(move |_| {
            p.popdown();
            s.emit(msg());
        });
        b.append(&item);
    };

    if t.playable {
        let tr = t.clone();
        add(&main, "Play next", icons::QUEUE_PLAY_NEXT_SYMBOLIC, Box::new(move || AppMsg::PlayNext(tr.clone())));
        let tr = t.clone();
        add(&main, "Add to queue", icons::QUEUE_MUSIC_SYMBOLIC, Box::new(move || AppMsg::AddToQueue(tr.clone())));
        let id = t.video_id.clone();
        add(&main, "Start radio", icons::PLAYLIST_PLAY_SYMBOLIC, Box::new(move || AppMsg::Radio(id.clone())));
        main.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    if let Some(a) = t.artists.iter().find(|a| a.browse_id.is_some()) {
        let id = a.browse_id.clone().unwrap();
        add(
            &main,
            "Go to artist",
            icons::PERSON_SYMBOLIC,
            Box::new(move || {
                AppMsg::Open(Target::Browse { id: id.clone(), params: None, page_type: PageType::Artist })
            }),
        );
    }
    if let Some(id) = t.album.as_ref().and_then(|a| a.browse_id.clone()) {
        add(
            &main,
            "Go to album",
            icons::ALBUM_SYMBOLIC,
            Box::new(move || AppMsg::Open(Target::Browse { id: id.clone(), params: None, page_type: PageType::Album })),
        );
    }

    // "Add to playlist" opens the second page.
    let playlists = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let back = menu_item("Back", icons::CHEVRON_LEFT_SYMBOLIC);
    {
        let st = stack.clone();
        back.connect_clicked(move |_| st.set_visible_child_name("main"));
    }
    playlists.append(&back);
    playlists.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let tr = t.clone();
    add(&playlists, "New playlist…", icons::ADD_SYMBOLIC, Box::new(move || AppMsg::NewPlaylist(Some(tr.clone()))));
    let list_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for (id, title) in ctx.own_playlists.iter() {
        let (tr, id) = (t.clone(), id.clone());
        add(
            &list_box,
            title,
            icons::LIBRARY_MUSIC_SYMBOLIC,
            Box::new(move || AppMsg::AddToPlaylist { playlist_id: id.clone(), track: tr.clone() }),
        );
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_max_content_height(320);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list_box));
    playlists.append(&scroll);

    if t.playable {
        let open = menu_item("Add to playlist", icons::LIBRARY_ADD_SYMBOLIC);
        let st = stack.clone();
        open.connect_clicked(move |_| st.set_visible_child_name("playlists"));
        main.append(&open);
    }
    if let (Some(pl), Some(_)) = (&ctx.editable_playlist, &t.set_video_id) {
        let (pl, tr) = (pl.clone(), t.clone());
        add(
            &main,
            "Remove from this playlist",
            icons::DELETE_SYMBOLIC,
            Box::new(move || AppMsg::RemoveFromPlaylist { playlist_id: pl.clone(), track: tr.clone() }),
        );
    }
    main.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let liked = t.like == Some(LikeStatus::Like);
    let tr = t.clone();
    add(
        &main,
        if liked { "Remove from Liked" } else { "Like" },
        if liked { icons::FAVORITE_FILL_SYMBOLIC } else { icons::FAVORITE_SYMBOLIC },
        Box::new(move || AppMsg::Rate(tr.clone(), if liked { LikeStatus::Indifferent } else { LikeStatus::Like })),
    );
    if ctx.downloaded.contains(&t.video_id) {
        let id = t.video_id.clone();
        add(&main, "Delete download", icons::DELETE_SYMBOLIC, Box::new(move || AppMsg::DeleteDownload(id.clone())));
    } else if t.playable && ctx.downloads_enabled {
        let tr = t.clone();
        add(&main, "Download", icons::DOWNLOAD_SYMBOLIC, Box::new(move || AppMsg::Download(vec![tr.clone()])));
    }

    stack.add_named(&main, Some("main"));
    stack.add_named(&playlists, Some("playlists"));
    pop.set_child(Some(&stack));
    pop
}

/// Small popover asking for a line of text (rename, new playlist).
pub fn ask_text(anchor: &impl IsA<gtk::Widget>, title: &str, initial: &str, on_ok: impl Fn(String) + 'static) {
    let pop = gtk::Popover::new();
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.set_margin_top(8);
    b.set_margin_bottom(8);
    b.set_margin_start(8);
    b.set_margin_end(8);
    let l = gtk::Label::new(Some(title));
    l.add_css_class("headline");
    l.set_xalign(0.0);
    let entry = gtk::Entry::new();
    entry.set_text(initial);
    entry.set_width_chars(28);
    entry.update_property(&[gtk::accessible::Property::Label(title)]);
    let ok = gtk::Button::with_label("Save");
    ok.add_css_class("pill");
    ok.add_css_class("primary");
    ok.set_halign(gtk::Align::End);
    b.append(&l);
    b.append(&entry);
    b.append(&ok);
    pop.set_child(Some(&b));
    pop.set_parent(anchor.as_ref());
    let on_ok = Rc::new(on_ok);
    let submit = {
        let (pop, entry, on_ok) = (pop.clone(), entry.clone(), on_ok.clone());
        move || {
            let text = entry.text().trim().to_string();
            if !text.is_empty() {
                on_ok(text);
                pop.popdown();
            }
        }
    };
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit());
    }
    ok.connect_clicked(move |_| submit());
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
    entry.grab_focus();
}

pub fn confirm(anchor: &impl IsA<gtk::Widget>, question: &str, action: &str, on_ok: impl Fn() + 'static) {
    let pop = gtk::Popover::new();
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.set_margin_top(8);
    b.set_margin_bottom(8);
    b.set_margin_start(8);
    b.set_margin_end(8);
    b.append(&gtk::Label::new(Some(question)));
    let ok = gtk::Button::with_label(action);
    ok.add_css_class("pill");
    ok.add_css_class("destructive");
    let p = pop.clone();
    ok.connect_clicked(move |_| {
        on_ok();
        p.popdown();
    });
    b.append(&ok);
    pop.set_child(Some(&b));
    pop.set_parent(anchor.as_ref());
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

#[cfg(test)]
mod tests {
    #[test]
    fn folding() {
        assert_eq!(super::fold("Şarkı İstanbul Gönül"), "sarki istanbul gonul");
        assert_eq!(super::fold("ÇAĞRI"), "cagri");
    }
}
