//! Full-size "Now playing" view: large cover, title, artist, album over a cover-colored glow.
//! Lyrics show in the side panel next to it.

use crate::icon_names as icons;
use crate::images;
use crate::msg::AppMsg;
use crate::widgets::icon_button;
use gtk::glib;
use gtk::prelude::*;
use relm4::Sender;
use sonora_ytm::{PageType, Target, Track};

const COVER: i32 = 380;

pub struct NowPlaying {
    pub root: gtk::Revealer,
    cover: gtk::Image,
    title: gtk::Label,
    artist: gtk::Label,
    album: gtk::Label,
    pub shown: bool,
}

impl NowPlaying {
    pub fn new(sender: &Sender<AppMsg>) -> Self {
        let root = gtk::Revealer::new();
        root.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        root.set_transition_duration(200);
        root.set_can_target(false);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
        page.add_css_class("now-playing-view");
        root.set_child(Some(&page));

        let close = icon_button(icons::KEYBOARD_ARROW_DOWN_SYMBOLIC, "Close (Esc)");
        close.add_css_class("back");
        close.set_halign(gtk::Align::Start);
        {
            let s = sender.clone();
            close.connect_clicked(move |_| s.emit(AppMsg::ToggleNowPlaying));
        }
        page.append(&close);

        let center = gtk::Box::new(gtk::Orientation::Vertical, 14);
        center.set_valign(gtk::Align::Center);
        center.set_halign(gtk::Align::Center);
        center.set_vexpand(true);

        let img = gtk::Image::new();
        img.set_pixel_size(COVER);
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        frame.add_css_class("np-cover");
        frame.set_overflow(gtk::Overflow::Hidden);
        frame.set_halign(gtk::Align::Center);
        frame.append(&img);
        img.update_property(&[gtk::accessible::Property::Label("Album cover")]);

        let title = gtk::Label::builder()
            .css_classes(["np-title"])
            .accessible_role(gtk::AccessibleRole::Heading)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .max_width_chars(32)
            .build();
        let artist = link_label("np-artist", sender);
        let album = link_label("np-album", sender);

        center.append(&frame);
        center.append(&title);
        center.append(&artist);
        center.append(&album);
        page.append(&center);

        Self { root, cover: img, title, artist, album, shown: false }
    }

    pub fn set_track(&self, t: Option<&Track>) {
        let Some(t) = t else {
            self.title.set_label("Not playing");
            self.artist.set_label("");
            self.album.set_label("");
            images::load(&self.cover, None, COVER as u32);
            return;
        };
        self.title.set_label(&t.title);
        let artists: Vec<String> = t.artists.iter().map(|a| link(&a.name, a.browse_id.as_deref())).collect();
        self.artist.set_markup(&artists.join(", "));
        match &t.album {
            Some(a) => self.album.set_markup(&link(&a.name, a.browse_id.as_deref())),
            None => self.album.set_label(""),
        }
        images::load(&self.cover, t.thumbnail.as_deref(), COVER as u32);
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
        self.root.set_reveal_child(shown);
        self.root.set_can_target(shown);
    }
}

fn link(name: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => {
            format!("<a href=\"browse:{}\">{}</a>", glib::markup_escape_text(id), glib::markup_escape_text(name))
        }
        None => glib::markup_escape_text(name).to_string(),
    }
}

fn link_label(class: &str, sender: &Sender<AppMsg>) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class(class);
    l.set_justify(gtk::Justification::Center);
    l.set_wrap(true);
    let s = sender.clone();
    l.connect_activate_link(move |_, uri| {
        if let Some(id) = uri.strip_prefix("browse:") {
            s.emit(AppMsg::ToggleNowPlaying);
            s.emit(AppMsg::Open(Target::Browse { id: id.to_string(), params: None, page_type: PageType::from_id(id) }));
        }
        glib::Propagation::Stop
    });
    l
}
