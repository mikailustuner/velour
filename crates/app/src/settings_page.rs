//! Settings page.

use crate::eq;
use crate::msg::{AppMsg, SettingChange};
use crate::page_view::large_title;
use crate::settings::{Settings, Theme};
use gtk::prelude::*;
use relm4::Sender;

pub fn build(
    s: &Settings,
    sender: &Sender<AppMsg>,
    account: &str,
    health: Option<&velour_player::Health>,
) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 28);
    page.add_css_class("page");
    page.append(&large_title("Settings"));

    // ---- appearance & playback ----
    let g = group(&page, "Appearance & playback");
    let theme = gtk::DropDown::from_strings(&["Dark", "Light", "Follow system"]);
    theme.set_selected(match s.theme {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::System => 2,
    });
    {
        let s = sender.clone();
        theme.connect_selected_notify(move |d| {
            let t = match d.selected() {
                1 => Theme::Light,
                2 => Theme::System,
                _ => Theme::Dark,
            };
            s.emit(AppMsg::SetTheme(t));
        });
    }
    row(&g, "Theme", None, &theme);

    let names = eq::names();
    let eqd = gtk::DropDown::from_strings(&names);
    eqd.set_selected(names.iter().position(|n| *n == s.eq_preset).unwrap_or(0) as u32);
    {
        let s = sender.clone();
        eqd.connect_selected_notify(move |d| {
            if let Some(name) = eq::names().get(d.selected() as usize) {
                s.emit(AppMsg::Settings(SettingChange::Eq(name.to_string())));
            }
        });
    }
    row(&g, "Equalizer", Some("Applied instantly to whatever is playing."), &eqd);

    switch(
        &g,
        "Show playlists in the sidebar",
        Some("Turn off to keep playlist names private, e.g. while sharing your screen."),
        s.sidebar_playlists,
        sender,
        SettingChange::SidebarPlaylists,
    );
    switch(
        &g,
        "Keep playing when the window is closed",
        Some("Run Velour again to bring the window back."),
        s.close_to_background,
        sender,
        SettingChange::CloseToBackground,
    );
    switch(
        &g,
        "Add plays to YouTube Music history",
        Some("Helps YouTube Music recommendations. Counted after 30 seconds."),
        s.send_history,
        sender,
        SettingChange::SendHistory,
    );

    switch(
        &g,
        "Low memory mode",
        Some("Software rendering: about 160 MB less memory, but scrolling uses much more CPU. Applies after restart."),
        s.low_memory,
        sender,
        SettingChange::LowMemory,
    );

    // ---- desktop ----
    let g = group(&page, "Desktop");
    switch(
        &g,
        "Notification on song change",
        Some("Needs a notification daemon (mako, dunst, swaync…)."),
        s.notifications,
        sender,
        SettingChange::Notifications,
    );
    switch(&g, "Tray icon", Some("Needs a bar with a tray (e.g. waybar)."), s.tray, sender, SettingChange::Tray);
    switch(
        &g,
        "Discord status",
        Some("Shows the current song on your Discord profile."),
        s.discord,
        sender,
        SettingChange::Discord,
    );
    let app_id = gtk::Entry::new();
    app_id.set_text(&s.discord_app_id);
    app_id.set_placeholder_text(Some("Discord application ID"));
    app_id.set_width_chars(22);
    app_id.update_property(&[gtk::accessible::Property::Label("Discord application ID")]);
    {
        let s = sender.clone();
        app_id.connect_activate(move |e| s.emit(AppMsg::Settings(SettingChange::DiscordAppId(e.text().to_string()))));
        let s = sender.clone();
        app_id.connect_has_focus_notify(move |e| {
            if !e.has_focus() {
                s.emit(AppMsg::Settings(SettingChange::DiscordAppId(e.text().to_string())));
            }
        });
    }
    row(&g, "Discord application ID", Some("Create one at discord.com/developers → Applications."), &app_id);

    // ---- storage ----
    let g = group(&page, "Storage");
    switch(
        &g,
        "Offline downloads",
        Some(
            "Save songs for offline listening. Only for content you have the right to keep, for personal use; check YouTube’s terms first.",
        ),
        s.downloads,
        sender,
        SettingChange::Downloads,
    );
    let dir = gtk::Button::with_label(&s.download_dir.display().to_string());
    dir.add_css_class("pill");
    {
        let s = sender.clone();
        dir.connect_clicked(move |b| {
            let dialog = gtk::FileDialog::builder().title("Download folder").modal(true).build();
            let (s, b2) = (s.clone(), b.clone());
            let window = b.root().and_downcast::<gtk::Window>();
            dialog.select_folder(window.as_ref(), gtk::gio::Cancellable::NONE, move |r| {
                if let Some(path) = r.ok().and_then(|f| f.path()) {
                    b2.set_label(&path.display().to_string());
                    s.emit(AppMsg::Settings(SettingChange::DownloadDir(path)));
                }
            });
        });
    }
    row(&g, "Download folder", None, &dir);
    let clear = gtk::Button::with_label("Clear image cache");
    clear.add_css_class("pill");
    {
        let s = sender.clone();
        clear.connect_clicked(move |_| s.emit(AppMsg::Settings(SettingChange::ClearCache)));
    }
    row(&g, "Cover art cache", Some("Downloaded covers older than 30 days are removed automatically."), &clear);

    // ---- playback helpers ----
    let g = group(&page, "Playback helpers");
    let (version_text, can_update) = match health {
        None => ("Checking…".to_string(), false),
        Some(h) => match (&h.yt_dlp_version, h.yt_dlp_age_days) {
            (Some(v), Some(age)) => (format!("Version {v} ({age} days old)"), h.updatable),
            (Some(v), None) => (format!("Version {v}"), h.updatable),
            (None, _) => ("Not found — install yt-dlp to play music".to_string(), false),
        },
    };
    let update = gtk::Button::with_label("Update");
    update.add_css_class("pill");
    update.set_sensitive(can_update);
    if !can_update && health.is_some_and(|h| h.yt_dlp_version.is_some()) {
        update.set_tooltip_text(Some("Installed by the system package manager; update it there."));
    }
    {
        let s = sender.clone();
        update.connect_clicked(move |_| s.emit(AppMsg::Settings(SettingChange::UpdateYtDlp)));
    }
    row(&g, "yt-dlp", Some(&version_text), &update);
    let js = match health {
        Some(h) if h.js_runtime => "Found",
        Some(_) => "Missing — install deno for reliable playback",
        None => "Checking…",
    };
    row(&g, "JavaScript runtime (deno)", Some(js), &gtk::Box::new(gtk::Orientation::Horizontal, 0));

    // ---- account ----
    let g = group(&page, "Account");
    let browsers = velour_ytm::Cookies::BROWSERS;
    let names: Vec<String> = browsers.iter().map(|b| capitalize(b)).collect();
    let names_ref: Vec<&str> = names.iter().map(String::as_str).collect();
    let browser = gtk::DropDown::from_strings(&names_ref);
    browser.set_selected(browsers.iter().position(|b| *b == s.browser).unwrap_or(0) as u32);
    {
        let s = sender.clone();
        browser.connect_selected_notify(move |d| {
            if let Some(b) = velour_ytm::Cookies::BROWSERS.get(d.selected() as usize) {
                s.emit(AppMsg::Settings(SettingChange::Browser(b.to_string())));
            }
        });
    }
    let who = if account.is_empty() { "Not signed in".to_string() } else { format!("Signed in as {account}") };
    row(
        &g,
        "Sign in with browser",
        Some(&format!("{who}. Velour uses the browser’s YouTube Music session; nothing is stored or sent elsewhere.")),
        &browser,
    );

    // ---- about ----
    let g = group(&page, "About");
    row(
        &g,
        &format!("Velour {}", env!("CARGO_PKG_VERSION")),
        Some(
            "Unofficial, open-source player (GPL-3.0). Not affiliated with, endorsed or sponsored by YouTube or Google. Synced lyrics: LRCLIB.\n\nSpace play/pause · Ctrl+←/→ previous/next · Shift+←/→ seek · Ctrl+↑/↓ volume · M mute · P now playing · Ctrl+L like · Ctrl+F search · Alt+← back · Ctrl+Q quit",
        ),
        &gtk::Box::new(gtk::Orientation::Horizontal, 0),
    );

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&page));
    scroll.upcast()
}

fn group(page: &gtk::Box, title: &str) -> gtk::ListBox {
    let l = gtk::Label::builder()
        .label(title)
        .css_classes(["section-title"])
        .xalign(0.0)
        .accessible_role(gtk::AccessibleRole::Heading)
        .build();
    let list = gtk::ListBox::new();
    list.add_css_class("settings-group");
    list.set_selection_mode(gtk::SelectionMode::None);
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.set_width_request(360);
    b.set_halign(gtk::Align::Fill);
    b.append(&l);
    b.append(&list);
    page.append(&b);
    list
}

fn row(list: &gtk::ListBox, title: &str, subtitle: Option<&str>, control: &impl IsA<gtk::Widget>) {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    b.add_css_class("settings-row");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.set_wrap(true);
    text.append(&t);
    if let Some(sub) = subtitle {
        let st = gtk::Label::new(Some(sub));
        st.add_css_class("dim");
        st.add_css_class("caption");
        st.set_xalign(0.0);
        st.set_wrap(true);
        text.append(&st);
    }
    control.as_ref().set_valign(gtk::Align::Center);
    control.as_ref().update_property(&[gtk::accessible::Property::Label(title)]);
    b.append(&text);
    b.append(control);
    let r = gtk::ListBoxRow::new();
    r.set_activatable(false);
    r.set_child(Some(&b));
    list.append(&r);
}

fn switch(
    list: &gtk::ListBox,
    title: &str,
    subtitle: Option<&str>,
    value: bool,
    sender: &Sender<AppMsg>,
    make: fn(bool) -> SettingChange,
) {
    let sw = gtk::Switch::new();
    sw.set_active(value);
    let s = sender.clone();
    sw.connect_active_notify(move |w| s.emit(AppMsg::Settings(make(w.is_active()))));
    row(list, title, subtitle, &sw);
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
