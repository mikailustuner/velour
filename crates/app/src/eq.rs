//! Equalizer presets as mpv `af` filter chains (lavfi peaking EQ bands).

/// (name, [(frequency Hz, gain dB)])
pub const PRESETS: &[(&str, &[(u32, i32)])] = &[
    ("Flat", &[]),
    ("Bass boost", &[(60, 6), (150, 3), (400, -1)]),
    ("Treble boost", &[(4000, 2), (8000, 4), (14000, 5)]),
    ("Vocal", &[(100, -2), (1000, 2), (3000, 4), (6000, 2)]),
    ("Loudness", &[(60, 4), (150, 2), (8000, 2), (14000, 3)]),
    ("Acoustic", &[(100, 2), (500, 1), (2500, 2), (8000, 3)]),
];

pub fn names() -> Vec<&'static str> {
    PRESETS.iter().map(|(n, _)| *n).collect()
}

/// mpv `af` value for a preset; empty string means no filter.
pub fn filter(name: &str) -> String {
    let bands = PRESETS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b).unwrap_or(&[]);
    if bands.is_empty() {
        return String::new();
    }
    let chain: Vec<String> = bands.iter().map(|(f, g)| format!("equalizer=f={f}:t=q:w=1:g={g}")).collect();
    // Small pre-gain cut avoids clipping on boosted presets.
    format!("lavfi=[{},volume=-3dB]", chain.join(","))
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_chain() {
        assert_eq!(super::filter("Flat"), "");
        assert_eq!(super::filter("unknown"), "");
        assert_eq!(
            super::filter("Bass boost"),
            "lavfi=[equalizer=f=60:t=q:w=1:g=6,equalizer=f=150:t=q:w=1:g=3,equalizer=f=400:t=q:w=1:g=-1,volume=-3dB]"
        );
    }
}
