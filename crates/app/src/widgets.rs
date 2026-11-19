//! Small widget helpers shared across views.

use gtk::prelude::*;

/// Icon-only button with tooltip and accessible label (icon-only controls need both).
pub fn icon_button(icon: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("icon");
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// Updates an icon button's icon, tooltip and accessible label together.
pub fn set_icon_button(b: &gtk::Button, icon: &str, label: &str) {
    b.set_icon_name(icon);
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
}

/// 194 → "3:14", 3723 → "1:02:03".
pub fn fmt_duration(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Small square cover: a fixed-size image clipped to rounded corners by its frame.
pub fn small_cover(px: i32, class: &str) -> (gtk::Box, gtk::Image) {
    let img = gtk::Image::new();
    img.set_pixel_size(px);
    let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.add_css_class(class);
    frame.set_valign(gtk::Align::Center);
    frame.set_halign(gtk::Align::Start);
    frame.append(&img);
    (frame, img)
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations() {
        assert_eq!(super::fmt_duration(0), "0:00");
        assert_eq!(super::fmt_duration(194), "3:14");
        assert_eq!(super::fmt_duration(3723), "1:02:03");
    }
}
