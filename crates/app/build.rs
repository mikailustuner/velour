fn main() {
    // Icons are Material Symbols copied into ./icons so one consistent set is used
    // (the shipped sets overlap on names like `explore` and `person`).
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("dev.sonora.Sonora"),
        None::<&str>,
        Some("icons"),
        [] as [&str; 0],
    );
}
