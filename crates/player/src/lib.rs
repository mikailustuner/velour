//! Audio playback for Sonora: libmpv for decoding/output, yt-dlp for stream URLs.

mod resolver;

pub use resolver::{Health, Resolver, looks_outdated};

use anyhow::{Result, anyhow};
use libmpv2::{Format, Mpv, events::Event as MpvEvent, events::PropertyData};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A new entry started; `id` is what was passed to `play`/`set_next`.
    Started {
        id: String,
    },
    Position(f64),
    Duration(f64),
    Paused(bool),
    Volume(f64),
    /// Current entry finished. `eof` is false when stopped/replaced.
    Ended {
        eof: bool,
    },
    /// Nothing left to play.
    Idle,
    Error(String),
}

const END_EOF: u32 = 0;

/// Thread-safe handle; mpv's client API may be called from any thread.
#[derive(Clone)]
pub struct Player {
    mpv: Arc<Mpv>,
}

impl Player {
    /// `on_event` runs on the player's event thread. `ao` overrides the audio output
    /// (e.g. `null` for tests).
    pub fn new(ao: Option<&str>, on_event: impl Fn(Event) + Send + 'static) -> Result<Self> {
        // libmpv refuses to start unless numbers are formatted the C way; GTK sets the
        // user's locale (e.g. tr_TR uses a decimal comma), so reset just LC_NUMERIC.
        // SAFETY: called before any other thread formats numbers via libc.
        unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };
        let mpv = Mpv::with_initializer(|init| {
            if let Some(ao) = ao {
                init.set_property("ao", ao)?;
            }
            for (k, v) in [
                ("vid", "no"),
                ("video", "no"),
                ("audio-display", "no"),
                ("ytdl", "no"),
                ("idle", "yes"),
                ("gapless-audio", "weak"),
                ("prefetch-playlist", "yes"),
                // Keep network buffers small: this is audio, not 4K video.
                ("cache", "yes"),
                ("demuxer-max-bytes", "16MiB"),
                ("demuxer-max-back-bytes", "4MiB"),
                ("network-timeout", "20"),
                ("audio-client-name", "Sonora"),
                ("volume-max", "100"),
            ] {
                init.set_property(k, v)?;
            }
            Ok(())
        })
        .map_err(|e| anyhow!("mpv init: {e:?}"))?;
        let mpv = Arc::new(mpv);

        for (name, fmt) in [
            ("time-pos", Format::Double),
            ("duration", Format::Double),
            ("pause", Format::Flag),
            ("volume", Format::Double),
            ("idle-active", Format::Flag),
        ] {
            mpv.observe_property(name, fmt, 0).map_err(|e| anyhow!("observe {name}: {e:?}"))?;
        }

        let events = mpv.clone();
        std::thread::Builder::new().name("sonora-mpv-events".into()).spawn(move || {
            let mut last_pos = -1.0f64;
            let mut last_emit = Instant::now();
            loop {
                let Some(ev) = events.wait_event(60.0) else { continue };
                match ev {
                    Ok(MpvEvent::FileLoaded) => {
                        let id: String = events.get_property("force-media-title").unwrap_or_default();
                        last_pos = -1.0;
                        on_event(Event::Started { id });
                    }
                    Ok(MpvEvent::EndFile(reason)) => on_event(Event::Ended { eof: reason == END_EOF }),
                    Ok(MpvEvent::PropertyChange { name, change, .. }) => match (name, change) {
                        ("time-pos", PropertyData::Double(p)) => {
                            // Throttle to ~1/s: the UI only shows whole seconds (synced lyrics
                            // allow ~0.15 s lead, so 1 s steps are enough).
                            if (p - last_pos).abs() >= 1.0 || last_emit.elapsed() > Duration::from_millis(1500) {
                                last_pos = p;
                                last_emit = Instant::now();
                                on_event(Event::Position(p));
                            }
                        }
                        ("duration", PropertyData::Double(d)) => on_event(Event::Duration(d)),
                        ("pause", PropertyData::Flag(p)) => on_event(Event::Paused(p)),
                        ("volume", PropertyData::Double(v)) => on_event(Event::Volume(v)),
                        ("idle-active", PropertyData::Flag(true)) => on_event(Event::Idle),
                        _ => {}
                    },
                    Ok(MpvEvent::Shutdown) => break,
                    Ok(_) => {}
                    Err(e) => {
                        // libmpv2 reports a failed EndFile (reason ERROR) as Err.
                        on_event(Event::Error(format!("{e:?}")));
                        on_event(Event::Ended { eof: false });
                    }
                }
            }
        })?;
        Ok(Self { mpv })
    }

    fn cmd(&self, name: &str, args: &[&str]) -> Result<()> {
        self.mpv.command(name, args).map_err(|e| anyhow!("{name}: {e:?}"))
    }

    /// Replaces everything and starts `url` (stream URL or local path).
    pub fn play(&self, url: &str, id: &str) -> Result<()> {
        self.cmd("loadfile", &[url, "replace", "-1", &format!("force-media-title={id}")])?;
        self.set_pause(false)
    }

    /// Queues `url` right after the current entry so mpv can prefetch it and switch gaplessly.
    pub fn set_next(&self, url: &str, id: &str) -> Result<()> {
        self.clear_next()?;
        self.cmd("loadfile", &[url, "append", "-1", &format!("force-media-title={id}")])
    }

    /// Drops prepared entries after the current one.
    pub fn clear_next(&self) -> Result<()> {
        self.cmd("playlist-clear", &[])
    }

    pub fn set_pause(&self, paused: bool) -> Result<()> {
        self.mpv.set_property("pause", paused).map_err(|e| anyhow!("pause: {e:?}"))
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.cmd("cycle", &["pause"])
    }

    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.cmd("seek", &[&format!("{seconds:.3}"), "absolute"])
    }

    pub fn stop(&self) -> Result<()> {
        self.cmd("stop", &[])
    }

    /// 0–100.
    pub fn set_volume(&self, volume: f64) -> Result<()> {
        self.mpv.set_property("volume", volume.clamp(0.0, 100.0)).map_err(|e| anyhow!("volume: {e:?}"))
    }

    pub fn set_mute(&self, mute: bool) -> Result<()> {
        self.mpv.set_property("mute", mute).map_err(|e| anyhow!("mute: {e:?}"))
    }

    /// mpv `af` filter chain, e.g. `lavfi=[equalizer=f=60:t=q:w=1:g=4]`; empty clears it.
    pub fn set_audio_filter(&self, af: &str) -> Result<()> {
        self.mpv.set_property("af", af).map_err(|e| anyhow!("af: {e:?}"))
    }

    pub fn position(&self) -> f64 {
        self.mpv.get_property("time-pos").unwrap_or(0.0)
    }
}
