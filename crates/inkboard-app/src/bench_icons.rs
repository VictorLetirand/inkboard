//! Large-board performance check with icons (`inkboard --bench-icons COUNT[:DISTINCT]`): a board of COUNT icon images (DISTINCT different
//! ones, repeated) is built from the icon library, then a scripted session runs — first views, zooming in and out, panning, enlarging an
//! icon — while the time of every frame is recorded. The report tells whether the interface stays fluid (frames under 16.7 ms) and what
//! the SVG rasterization cost.

use crate::app::App;
use crate::platform::Instant;
use crate::render::TextureStats;
use eframe::egui;
use inkboard_core::num;
use inkboard_core::{AssetId, Document, Frame, Id, ImageObj, Kind, Object, Pt, Style, View};
use inkboard_icons::{IconRef, Library};

/// Phases of the scripted session: name and number of frames.
const PHASES: [(&str, usize); 6] =
    [("first views (fit all)", 40), ("fit all, steady", 60), ("zoom in to 300 %", 150), ("pan at 100 %", 150), ("zoom out", 150), ("enlarge one icon", 90)];

/// Builds a board of `count` icon objects using `distinct` different icons of the library, on a grid.
pub fn document(lib: &Library, count: usize, distinct: usize) -> Document {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let total = lib.icon_count();
    // `distinct` icons spread over the whole library (every set contributes, big and small ones)
    let mut picks: Vec<IconRef> = Vec::new();
    while picks.len() < distinct.max(1) {
        let mut k = usize::try_from(rnd() % total as u64).expect("below the icon count");
        for (si, s) in lib.sets.iter().enumerate() {
            if k < s.names.len() {
                picks.push(IconRef { set: si, icon: k });
                break;
            }
            k -= s.names.len();
        }
    }
    let mut doc = Document::new("Icon bench");
    let assets: Vec<(AssetId, bool, f64, f64)> = picks
        .iter()
        .filter_map(|r| {
            let svg = lib.svg(*r)?;
            let (w, h) = inkboard_icons::svg_dimensions(&svg).unwrap_or((24.0, 24.0));
            let aid = doc.add_asset("image/svg+xml", svg.into_bytes());
            Some((aid, lib.sets[r.set].mono, w, h))
        })
        .collect();
    let cols = num::to_usize(num::f64_of(count).sqrt().ceil());
    let mut objs = Vec::with_capacity(count);
    for i in 0..count {
        let (aid, mono, w, h) = &assets[usize::try_from(rnd() % assets.len() as u64).expect("below the asset count")];
        let k = (64.0 + f64::from(u8::try_from(rnd() % 64).expect("below 64"))) / w.max(*h);
        let frame = Frame::new(num::f64_of(i % cols) * 150.0, num::f64_of(i / cols) * 150.0, w * k, h * k);
        objs.push(Object::new(
            Kind::Image(ImageObj { frame, asset: aid.clone(), latex: None, mono: *mono, recolor: Vec::new() }),
            Style { width: 0.0, fill: None, ..Style::default() },
        ));
    }
    doc.pages[0].objects = objs;
    doc
}

/// State of the scripted session.
pub struct IconBench {
    frame: usize,
    last_start: Option<Instant>,
    /// Per phase: CPU time of each frame (ms) and interval between frames (ms).
    cpu: Vec<Vec<f64>>,
    interval: Vec<Vec<f64>>,
    stats_at_start: Vec<TextureStats>,
    fit: Option<View>,
    center: Option<Pt>,
    target: Option<Id>,
    pub count: usize,
    pub distinct: usize,
}

impl IconBench {
    pub fn new(count: usize, distinct: usize) -> Self {
        Self {
            frame: 0,
            last_start: None,
            cpu: vec![Vec::new(); PHASES.len()],
            interval: vec![Vec::new(); PHASES.len()],
            stats_at_start: vec![TextureStats::default(); PHASES.len() + 1],
            fit: None,
            center: None,
            target: None,
            count,
            distinct,
        }
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { 0.0 } else { sorted[num::to_usize((num::f64_of(sorted.len() - 1) * p).round())] }
}

fn summary(v: &[f64]) -> String {
    let mut t = v.to_vec();
    t.sort_by(f64::total_cmp);
    let mean = t.iter().sum::<f64>() / num::f64_of(t.len().max(1));
    let over = |lim: f64| 100.0 * num::f64_of(t.iter().filter(|x| **x > lim).count()) / num::f64_of(t.len().max(1));
    format!(
        "mean {mean:6.2} · p50 {:6.2} · p95 {:6.2} · p99 {:6.2} · max {:7.2} · >16.7 ms {:5.1} % · >33 ms {:5.1} %",
        pct(&t, 0.5),
        pct(&t, 0.95),
        pct(&t, 0.99),
        t.last().copied().unwrap_or(0.0),
        over(16.7),
        over(33.3)
    )
}

impl App {
    /// Called at the end of every frame while the icon bench runs.
    pub fn icon_bench_step(&mut self, ctx: &egui::Context, started: Instant) {
        let Some(mut bench) = self.icon_bench.take() else { return };
        ctx.request_repaint();
        let f = bench.frame;
        bench.frame += 1;
        let interval = bench.last_start.map(|t| started.duration_since(t).as_secs_f64() * 1000.0);
        bench.last_start = Some(started);
        if f == 0 {
            self.fit_pending = true;
            self.icon_bench = Some(bench);
            return;
        }
        if f == 3 {
            self.fit_view(false);
            bench.fit = Some(self.ed.page().view);
            bench.center = self.ed.page().content_bounds().map(|r| r.center());
        }
        // phase of this frame and its index inside the phase
        let mut start = 4;
        let (mut phase, mut k) = (PHASES.len(), 0);
        for (i, (_, len)) in PHASES.iter().enumerate() {
            if f < start + len {
                phase = i;
                k = f - start;
                break;
            }
            start += len;
        }
        if f >= 4 && phase < PHASES.len() {
            if k == 0 {
                bench.stats_at_start[phase] = self.textures.stats;
            }
            let (width, height) = (f64::from(self.canvas_size.0), f64::from(self.canvas_size.1));
            if let (Some(fit), Some(c)) = (bench.fit, bench.center) {
                let len = num::f64_of(PHASES[phase].1);
                let t = num::f64_of(k) / len;
                let at = |z: f64| View { x: width / 2.0 - c.x * z, y: height / 2.0 - c.y * z, zoom: z };
                match phase {
                    0 | 1 => self.set_view(fit),
                    2 => self.set_view(at(fit.zoom * (3.0 / fit.zoom).powf(t))),
                    3 => {
                        let v = at(1.0);
                        self.set_view(View { x: v.x - 12.0 * num::f64_of(k), y: v.y - 7.0 * num::f64_of(k), ..v });
                    }
                    4 => self.set_view(at(3.0 * (fit.zoom / 3.0).powf(t))),
                    _ => {
                        self.set_view(at(1.0));
                        if bench.target.is_none() {
                            bench.target = self
                                .ed
                                .page()
                                .objects
                                .iter()
                                .min_by(|x, y| {
                                    let d = |o: &Object| o.frame().map_or(f64::MAX, |fr| fr.center().dist(c));
                                    d(x).total_cmp(&d(y))
                                })
                                .map(|o| o.id.clone());
                        }
                        if let Some(id) = bench.target.clone() {
                            self.ed.update(&id, |o| {
                                if let Kind::Image(i) = &mut o.kind {
                                    let (cx, cy) = (i.frame.x + i.frame.w / 2.0, i.frame.y + i.frame.h / 2.0);
                                    i.frame.w *= 1.03;
                                    i.frame.h *= 1.03;
                                    i.frame.x = cx - i.frame.w / 2.0;
                                    i.frame.y = cy - i.frame.h / 2.0;
                                }
                            });
                        }
                    }
                }
            }
            // the first frames of a phase carry the cost of the transition; every frame counts
            bench.cpu[phase].push(started.elapsed().as_secs_f64() * 1000.0);
            if let Some(i) = interval {
                bench.interval[phase].push(i);
            }
        }
        if phase == PHASES.len() {
            bench.stats_at_start[PHASES.len()] = self.textures.stats;
            self.print_icon_bench(&bench);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.icon_bench = Some(bench);
    }

    /// Report printed at the end of the icon bench.
    fn print_icon_bench(&self, bench: &IconBench) {
        println!(
            "icon bench: {} icons on the board ({} different), window {:.0} x {:.0} points",
            bench.count, bench.distinct, self.canvas_size.0, self.canvas_size.1
        );
        println!("frame time (CPU of our update), in ms; 'interval' = time between frames (includes drawing and the display refresh)");
        for (i, (name, _)) in PHASES.iter().enumerate() {
            let (a, z) = (bench.stats_at_start[i], bench.stats_at_start[i + 1]);
            println!(
                "\
{name}"
            );
            println!("  cpu      {}", summary(&bench.cpu[i]));
            println!("  interval {}", summary(&bench.interval[i]));
            println!(
                "  svg rasterizations {} ({:.0} ms total), deferred requests {}, evictions {}",
                z.rasterized - a.rasterized,
                z.raster_ms - a.raster_ms,
                z.deferred - a.deferred,
                z.evicted - a.evicted
            );
        }
        println!(
            "\
textures held at the end: {} ({:.1} MB)",
            self.textures.texture_count(),
            num::f64_of(self.textures.bytes()) / (1024.0 * 1024.0)
        );
        println!("cache rebuild (once per modification): {:.1} ms", self.caches.build_ms);
    }
}
