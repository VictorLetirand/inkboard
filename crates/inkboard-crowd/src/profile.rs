//! Behavior of the simulated people (rates are per guest) and the minimal command-line parser shared by the tools.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Cursor messages per second while the cursor moves.
    pub cursor_hz: f64,
    /// Share of the time the cursor is moving (the rest of the time it rests; a resting cursor sends nothing).
    pub cursor_active: f64,
    /// Actions per minute (Poisson arrivals).
    pub stroke_per_min: f64,
    pub create_per_min: f64,
    pub move_per_min: f64,
    pub delete_per_min: f64,
    pub text_per_min: f64,
    pub reorder_per_min: f64,
    pub undo_per_min: f64,
    /// Image pastes per minute; `big_image_prob` of them are big (2 to 8 MiB), the others small (30 to 400 KiB).
    pub image_per_min: f64,
    pub big_image_prob: f64,
    /// Share of the guests that only look (read-only link).
    pub viewer_fraction: f64,
    /// Guests on a bad link: extra latency (each way), jitter, inbound frame loss, inbound bandwidth (kbit/s, 0 = unlimited).
    pub slow_fraction: f64,
    pub slow_delay_ms: u64,
    pub slow_jitter_ms: u64,
    pub slow_loss: f64,
    pub slow_kbps: u64,
    /// Guests that stop reading altogether some time after they joined (a frozen laptop): `stuck_after_s` after joining.
    pub stuck_fraction: f64,
    pub stuck_after_s: f64,
    /// People who leave (mean time on the board, seconds; 0 = nobody leaves) and come back later as somebody new.
    pub mean_stay_s: f64,
    pub rejoin_pause_s: (f64, f64),
    /// Connection drops per guest and hour (the guest comes back after a pause and resumes its place).
    pub drops_per_hour: f64,
    pub drop_pause_s: (f64, f64),
    /// Arrivals are spread over this many seconds.
    pub ramp_s: f64,
    /// Fetch the images of the board after joining (the real client does).
    pub fetch_images: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            cursor_hz: 30.0,
            cursor_active: 0.6,
            stroke_per_min: 2.0,
            create_per_min: 5.0,
            move_per_min: 4.0,
            delete_per_min: 1.0,
            text_per_min: 2.0,
            reorder_per_min: 0.3,
            undo_per_min: 0.8,
            image_per_min: 0.2,
            big_image_prob: 0.15,
            viewer_fraction: 0.2,
            slow_fraction: 0.1,
            slow_delay_ms: 200,
            slow_jitter_ms: 80,
            slow_loss: 0.02,
            slow_kbps: 512,
            stuck_fraction: 0.0,
            stuck_after_s: 30.0,
            mean_stay_s: 0.0,
            rejoin_pause_s: (1.0, 10.0),
            drops_per_hour: 0.0,
            drop_pause_s: (0.5, 4.0),
            ramp_s: 10.0,
            fetch_images: true,
        }
    }
}

impl Profile {
    /// A quiet session: only cursors and the odd edit (the typical "2 to 5 people" meeting).
    #[must_use]
    pub fn calm() -> Self {
        Self { stroke_per_min: 1.0, create_per_min: 2.0, move_per_min: 2.0, text_per_min: 1.0, image_per_min: 0.05, slow_fraction: 0.0, ..Self::default() }
    }

    /// Everything is turned up: a workshop where everybody draws at once.
    #[must_use]
    pub fn busy() -> Self {
        Self { stroke_per_min: 6.0, create_per_min: 12.0, move_per_min: 10.0, delete_per_min: 2.0, text_per_min: 4.0, image_per_min: 0.4, ..Self::default() }
    }

    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "default" | "workshop" => Some(Self::default()),
            "calm" => Some(Self::calm()),
            "busy" => Some(Self::busy()),
            _ => None,
        }
    }

    /// Overrides from `--key value` options.
    pub fn apply(&mut self, a: &Args) -> Result<(), String> {
        macro_rules! f64s {
            ($($name:ident => $key:literal),* $(,)?) => { $( if let Some(v) = a.parse::<f64>($key)? { self.$name = v; } )* };
        }
        f64s!(cursor_hz => "cursor-hz", cursor_active => "cursor-active", stroke_per_min => "strokes-per-min", create_per_min => "creates-per-min",
            move_per_min => "moves-per-min", delete_per_min => "deletes-per-min", text_per_min => "texts-per-min", reorder_per_min => "reorders-per-min",
            undo_per_min => "undos-per-min", image_per_min => "images-per-min", big_image_prob => "big-image-prob", viewer_fraction => "viewers",
            slow_fraction => "slow", slow_loss => "slow-loss", stuck_fraction => "stuck", stuck_after_s => "stuck-after", mean_stay_s => "mean-stay",
            drops_per_hour => "drops-per-hour", ramp_s => "ramp");
        if let Some(v) = a.parse::<u64>("slow-delay-ms")? {
            self.slow_delay_ms = v;
        }
        if let Some(v) = a.parse::<u64>("slow-jitter-ms")? {
            self.slow_jitter_ms = v;
        }
        if let Some(v) = a.parse::<u64>("slow-kbps")? {
            self.slow_kbps = v;
        }
        if a.flag("no-images") {
            self.fetch_images = false;
        }
        Ok(())
    }
}

/// `--key value` options and `--flag`s, plus positional words.
#[derive(Clone, Debug, Default)]
pub struct Args {
    pub words: Vec<String>,
    opts: BTreeMap<String, String>,
}

impl Args {
    pub fn new(items: impl IntoIterator<Item = String>) -> Self {
        let mut a = Self::default();
        let mut it = items.into_iter().peekable();
        while let Some(x) = it.next() {
            if let Some(key) = x.strip_prefix("--") {
                if let Some((k, v)) = key.split_once('=') {
                    a.opts.insert(k.to_owned(), v.to_owned());
                } else if it.peek().is_some_and(|n| !n.starts_with("--")) {
                    a.opts.insert(key.to_owned(), it.next().unwrap_or_default());
                } else {
                    a.opts.insert(key.to_owned(), String::new());
                }
            } else {
                a.words.push(x);
            }
        }
        a
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.opts.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn flag(&self, key: &str) -> bool {
        self.opts.contains_key(key)
    }

    pub fn parse<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, String> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.trim().parse::<T>().map(Some).map_err(|_| format!("--{key}: cannot read '{v}'")),
        }
    }

    pub fn or<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        Ok(self.parse(key)?.unwrap_or(default))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_override_the_profile() {
        let a = Args::new(["run", "--cursor-hz", "20", "--slow=0.5", "--no-images", "--slow-kbps", "128"].map(String::from));
        let mut p = Profile::default();
        p.apply(&a).unwrap();
        assert_eq!((p.cursor_hz, p.slow_fraction, p.slow_kbps, p.fetch_images), (20.0, 0.5, 128, false));
        assert_eq!(a.words, ["run"]);
        assert!(Args::new(["--x".to_owned(), "oops".to_owned()]).parse::<f64>("x").is_err());
    }
}
