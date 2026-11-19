mod app;
mod downloads;
mod eq;
mod images;
mod integrations;
mod lyrics;
mod msg;
mod now_playing;
mod page_view;
mod player_bar;
mod queue;
mod remote;
mod settings;
mod settings_page;
mod side_panel;
mod widgets;

// Some icons are reserved for later phases.
#[allow(dead_code)]
mod icon_names {
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
    pub use custom::*;
}

pub const APP_ID: &str = "io.github.mikailustuner.Velour";

fn main() {
    // Must be decided before GTK starts. An explicit GSK_RENDERER wins.
    if settings::Settings::load().low_memory && std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: single-threaded at this point.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
    let app = relm4::RelmApp::new(APP_ID);
    relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    app.run::<app::App>(());
}
