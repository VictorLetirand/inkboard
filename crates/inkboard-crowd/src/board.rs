//! Realistic seed boards: a mix of shapes, notes, rich texts, code blocks, formulas, icons, connectors, tables, pen strokes and
//! about fifty photos of very different sizes, spread over several pages and laid out in clusters. Same `BoardSpec`, same board.

use crate::png::noise_png_of_size;
use crate::rng::Rng;
use inkboard_core::connector::{Arrowhead, Connector, End, Route};
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::highlight::Lang;
use inkboard_core::model::{Asset, AssetId, BoxGeom, BoxObj, Document, ImageObj, Kind, Label, Object, Page, Table};
use inkboard_core::style::{Align, Color, Dash, FILLS, FontFamily, PALETTE, Style};
use inkboard_core::text::{RichText, Span, SpanStyle};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct BoardSpec {
    /// Objects in all, spread over the pages.
    pub objects: usize,
    pub pages: usize,
    /// Distinct photos (PNG noise of 20 KB to 6 MB, mostly small).
    pub photos: usize,
    pub seed: u64,
}

impl BoardSpec {
    /// The board of a given size with the usual proportions (a few pages for a big board).
    #[must_use]
    pub fn sized(objects: usize, seed: u64) -> Self {
        let pages = match objects {
            0..=1_500 => 2,
            1_501..=8_000 => 4,
            _ => 6,
        };
        Self { objects, pages, photos: 50, seed }
    }
}

const WORDS: &[&str] = &[
    "plan", "budget", "release", "sprint", "risk", "owner", "review", "design", "api", "latency", "cache", "queue", "deploy", "metrics", "customer", "roadmap",
    "backlog", "retro", "demo", "feedback", "pilot", "scope", "quality", "security", "cost", "launch", "team", "goal", "idea", "action", "next", "blocked",
    "done", "draft", "vote", "priority",
];

const CODE: &[&str] = &[
    "fn main() {\n    let mut total = 0u64;\n    for i in 0..100 {\n        total += i * i;\n    }\n    println!(\"{total}\");\n}",
    "def fib(n):\n    a, b = 0, 1\n    for _ in range(n):\n        a, b = b, a + b\n    return a\n\nprint(fib(30))",
    "SELECT user_id, COUNT(*) AS n\nFROM events\nWHERE ts > now() - interval '7 days'\nGROUP BY user_id\nORDER BY n DESC;",
    "const items = list.filter(x => x.done).map(x => ({ id: x.id, at: Date.now() }));\nconsole.log(items.length);",
    "{\n  \"name\": \"inkboard\",\n  \"version\": \"0.1.0\",\n  \"features\": [\"share\", \"web\"]\n}",
];

const FORMULAS: &[&str] =
    &["E = mc^2", "\\int_0^1 x^2 dx = \\frac{1}{3}", "\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}", "a^2 + b^2 = c^2", "\\nabla \\cdot E = \\rho / \\epsilon_0"];

fn sentence(rng: &mut Rng, words: usize) -> String {
    (0..words).map(|_| *rng.pick(WORDS)).collect::<Vec<_>>().join(" ")
}

/// A little SVG icon (one of `n` distinct ones); `mono` icons are white so that the stroke color tints them.
fn icon_svg(k: usize, mono: bool) -> String {
    let mut rng = Rng::stream(0x1c0, k as u64);
    let color = if mono { "#ffffff".to_owned() } else { format!("#{:02x}{:02x}{:02x}", rng.below(200), rng.below(200), rng.below(200)) };
    let mut path = String::new();
    for i in 0..(4 + k % 6) {
        let (x, y) = (2.0 + rng.f64() * 20.0, 2.0 + rng.f64() * 20.0);
        path.push_str(&format!("{}{x:.1} {y:.1} ", if i == 0 { "M" } else { "L" }));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\"><path d=\"{path}Z\" fill=\"{color}\" stroke=\"{color}\" stroke-width=\"1\"/><circle cx=\"12\" cy=\"12\" r=\"{}\" fill=\"none\" stroke=\"{color}\"/></svg>",
        3 + k % 7
    )
}

fn formula_svg(k: usize) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 120 30\" width=\"120\" height=\"30\"><text x=\"2\" y=\"22\" font-size=\"18\" fill=\"#ffffff\">f{k}(x)</text></svg>"
    )
}

fn style(rng: &mut Rng) -> Style {
    let stroke = *rng.pick(&PALETTE);
    let fill = rng.chance(0.55).then(|| *rng.pick(&FILLS));
    Style {
        stroke,
        fill,
        width: *rng.pick(&[1.0, 2.0, 3.0, 4.0]),
        dash: if rng.chance(0.1) { Dash::Dashed } else { Dash::Solid },
        opacity: 1.0,
        roundness: 0.0,
        ..Style::default()
    }
}

/// Rich text with several fragments (bold, italic, colored, highlighted).
pub fn rich_text(rng: &mut Rng, words: usize) -> RichText {
    let mut spans = Vec::new();
    let mut left = words;
    while left > 0 {
        let n = (1 + rng.below(6)).min(left);
        left -= n;
        let mut s = sentence(rng, n);
        s.push(' ');
        let mut st = SpanStyle::default();
        if rng.chance(0.25) {
            st.bold = true;
        }
        if rng.chance(0.15) {
            st.italic = true;
        }
        if rng.chance(0.15) {
            st.color = Some(*rng.pick(&PALETTE));
        }
        if rng.chance(0.08) {
            st.highlight = Some(*rng.pick(&FILLS));
        }
        spans.push(Span { text: s, style: st });
    }
    RichText(spans)
}

fn id(spec: &BoardSpec, n: usize) -> inkboard_core::Id {
    inkboard_core::Id(format!("s{:x}-{n}", spec.seed & 0xffff_ffff))
}

/// The photo sizes: many small ones, a few big (the sum is about 30 MB for 50 photos).
fn photo_bytes(k: usize, rng: &mut Rng) -> usize {
    match k % 10 {
        0 => 3_000_000 + rng.below(3_000_000),
        1 | 2 => 600_000 + rng.below(1_000_000),
        3..=6 => 100_000 + rng.below(250_000),
        _ => 20_000 + rng.below(60_000),
    }
}

/// Builds the seed document of `spec`.
#[must_use]
pub fn seed_document(spec: &BoardSpec) -> Document {
    let mut rng = Rng::new(spec.seed);
    let mut doc = Document::new("Load test board");
    doc.pages.clear();
    // assets: icons (colored and monochrome), formulas, photos. Their identifiers are fixed by the seed too.
    let mut icons: Vec<(AssetId, bool)> = Vec::new();
    for k in 0..40 {
        let mono = k % 2 == 0;
        let aid = id(spec, 900_000 + k);
        doc.assets.insert(aid.clone(), Asset { mime: "image/svg+xml".to_owned(), data: Arc::new(icon_svg(k, mono).into_bytes()) });
        icons.push((aid, mono));
    }
    let mut formulas: Vec<AssetId> = Vec::new();
    for k in 0..8 {
        let aid = id(spec, 910_000 + k);
        doc.assets.insert(aid.clone(), Asset { mime: "image/svg+xml".to_owned(), data: Arc::new(formula_svg(k).into_bytes()) });
        formulas.push(aid);
    }
    let mut photos: Vec<(AssetId, f64, f64)> = Vec::new();
    for k in 0..spec.photos {
        let bytes = photo_bytes(k, &mut rng);
        let png = noise_png_of_size(bytes, spec.seed ^ (k as u64 + 1));
        // the frame keeps the 4:3 ratio of the picture
        let w = 160.0 + rng.f64() * 380.0;
        let aid = id(spec, 920_000 + k);
        doc.assets.insert(aid.clone(), Asset { mime: "image/png".to_owned(), data: Arc::new(png) });
        photos.push((aid, w, w * 0.75));
    }

    let pages = spec.pages.max(1);
    for p in 0..pages {
        let mut page = Page::new(&format!("Page {}", p + 1));
        page.id = id(spec, 800_000 + p);
        let share = spec.objects / pages + usize::from(p < spec.objects % pages);
        fill_page(&mut page, share, p * 1_000_000, spec, &mut rng, &icons, &formulas, &photos);
        doc.pages.push(page);
    }
    doc
}

#[allow(clippy::too_many_arguments)]
fn fill_page(
    page: &mut Page,
    count: usize,
    base: usize,
    spec: &BoardSpec,
    rng: &mut Rng,
    icons: &[(AssetId, bool)],
    formulas: &[AssetId],
    photos: &[(AssetId, f64, f64)],
) {
    // clusters on a coarse grid: people work in areas
    let cols = ((count as f64).sqrt() / 4.0).ceil().max(2.0) as usize;
    let cell = 1_800.0;
    let mut anchors: Vec<(usize, Pt)> = Vec::new(); // objects connectors can bind to
    let mut n = 0usize;
    let next_id = |n: &mut usize| {
        *n += 1;
        id(spec, base + *n)
    };
    let mut pending_photos: Vec<usize> = (0..photos.len()).collect();
    while page.objects.len() < count {
        let k = page.objects.len();
        let cluster = k / 25;
        let (cx, cy) = ((cluster % cols) as f64 * cell, (cluster / cols) as f64 * cell);
        let at = Pt::new(cx + rng.f64() * (cell - 400.0), cy + rng.f64() * (cell - 300.0));
        let roll = rng.f64();
        let mut obj: Object = if roll < 0.28 {
            // shape or sticky note with a short text
            let geom = *rng.pick(&[BoxGeom::Rect, BoxGeom::Rect, BoxGeom::Ellipse, BoxGeom::Diamond, BoxGeom::Triangle, BoxGeom::Sticky, BoxGeom::Sticky]);
            let words = 1 + rng.below(6);
            let label = Label::new(&sentence(rng, words), 18.0, Align::Center);
            Object::new_box(geom, Frame::new(at.x, at.y, 120.0 + rng.f64() * 120.0, 70.0 + rng.f64() * 60.0), style(rng), label)
        } else if roll < 0.44 {
            // free rich text (a paragraph with formatting)
            let words = 8 + rng.below(70);
            let label = Label {
                text: rich_text(rng, words),
                family: *rng.pick(&[FontFamily::Sans, FontFamily::Serif, FontFamily::Hand]),
                size: 16.0,
                align: Align::Left,
            };
            let mut st = style(rng);
            st.fill = None;
            st.width = 0.0;
            Object::new_box(BoxGeom::Text, Frame::new(at.x, at.y, 260.0 + rng.f64() * 160.0, 40.0 + words as f64 * 2.0), st, label)
        } else if roll < 0.47 {
            // code block
            let text = rng.pick(CODE);
            let label = Label { text: RichText::plain(*text), family: FontFamily::Mono, size: 14.0, align: Align::Left };
            let mut o = Object::new_box(
                BoxGeom::Rect,
                Frame::new(at.x, at.y, 380.0, 190.0),
                Style { fill: Some(Color::rgb(0xf1, 0xf3, 0xf5)), ..Style::default() },
                label,
            );
            if let Kind::Box(b) = &mut o.kind {
                b.lang = Some(*rng.pick(&[Lang::Rust, Lang::Python, Lang::Sql, Lang::JavaScript, Lang::Json]));
            }
            o
        } else if roll < 0.61 && anchors.len() > 2 {
            // connector between two shapes already on the page
            // a connector links shapes of the same area (the last few made), like in a real diagram
            let near = anchors.len().min(12);
            let a = anchors[anchors.len() - 1 - rng.below(near)].0;
            let b = anchors[anchors.len() - 1 - rng.below(near)].0;
            let (ida, idb) = (page.objects[a].id.clone(), page.objects[b].id.clone());
            let mut c = Connector::new(End::bound(ida), End::bound(idb));
            c.route = *rng.pick(&[Route::Straight, Route::Elbow, Route::Curve]);
            c.head_end = Arrowhead::Arrow;
            if rng.chance(0.2) {
                c.label = Some(Label::new(&sentence(rng, 2), 14.0, Align::Center));
            }
            Object::new(Kind::Connector(c), Style { width: 2.0, fill: None, ..style(rng) })
        } else if roll < 0.73 {
            // pen stroke
            let pts_n = 40 + rng.below(360);
            let mut p = at;
            let (mut dx, mut dy) = (rng.range(-3.0, 3.0), rng.range(-3.0, 3.0));
            let pts: Vec<Pt> = (0..pts_n)
                .map(|_| {
                    dx = (dx + rng.range(-0.8, 0.8)).clamp(-6.0, 6.0);
                    dy = (dy + rng.range(-0.8, 0.8)).clamp(-6.0, 6.0);
                    p = Pt::new(p.x + dx, p.y + dy);
                    p
                })
                .collect();
            let hl = rng.chance(0.1);
            Object::new_pen(pts, hl, Style { fill: None, width: if hl { 14.0 } else { 3.0 }, ..style(rng) })
        } else if roll < 0.77 {
            // table
            let (rows, cols_n) = (2 + rng.below(6), 2 + rng.below(4));
            let mut t = Table::new(at, rows, cols_n);
            for c in &mut t.cells {
                let words = 1 + rng.below(4);
                *c = rich_text(rng, words);
            }
            Object::new(Kind::Table(t), Style::default())
        } else if roll < 0.90 {
            // icon
            let (aid, mono) = rng.pick(icons).clone();
            let s = 40.0 + rng.f64() * 60.0;
            Object::new(
                Kind::Image(ImageObj { frame: Frame::new(at.x, at.y, s, s), asset: aid, latex: None, mono, recolor: Vec::new() }),
                Style { width: 0.0, fill: None, ..style(rng) },
            )
        } else if roll < 0.93 {
            // formula
            let k = rng.below(formulas.len());
            let o = ImageObj {
                frame: Frame::new(at.x, at.y, 120.0, 30.0),
                asset: formulas[k].clone(),
                latex: Some(FORMULAS[k % FORMULAS.len()].to_owned()),
                mono: true,
                recolor: Vec::new(),
            };
            Object::new(Kind::Image(o), Style { width: 0.0, fill: None, ..style(rng) })
        } else {
            // photo: every photo is used at least once, then repeated
            let k = pending_photos.pop().unwrap_or_else(|| rng.below(photos.len().max(1)));
            if photos.is_empty() {
                continue;
            }
            let (aid, w, h) = photos[k].clone();
            Object::new(
                Kind::Image(ImageObj { frame: Frame::new(at.x, at.y, w, h), asset: aid, latex: None, mono: false, recolor: Vec::new() }),
                Style { width: 0.0, fill: None, ..style(rng) },
            )
        };
        obj.id = next_id(&mut n);
        if matches!(&obj.kind, Kind::Box(BoxObj { geom, .. }) if *geom != BoxGeom::Text) {
            anchors.push((page.objects.len(), at));
        }
        page.objects.push(obj);
    }
}

/// Sets the fields added by protocol v4 on a share of the objects (drop shadows, enlarged arrowheads, recolored SVG icons), with its own
/// random stream: the rest of the board stays the one `seed_document` made. Opt-in (the load-test host reads `CROWD_NEW_FIELDS`).
pub fn add_v4_fields(doc: &mut Document, seed: u64) {
    let mut rng = Rng::stream(seed, 0x5ad0);
    for o in doc.pages.iter_mut().flat_map(|p| p.objects.iter_mut()) {
        match &mut o.kind {
            Kind::Box(_) if rng.chance(0.4) => o.style.shadow = true,
            Kind::Connector(c) if rng.chance(0.6) => c.set_head_scale(0.5 + rng.f64() as f32 * 3.5),
            Kind::Image(i) if !i.mono && i.latex.is_none() && doc.assets.get(&i.asset).is_some_and(|a| a.mime == "image/svg+xml") && rng.chance(0.5) => {
                i.recolor = vec![(*rng.pick(&PALETTE), *rng.pick(&PALETTE)), (*rng.pick(&PALETTE), Color::Ink)];
            }
            _ => {}
        }
    }
}

/// Total bytes of the images of a document.
#[must_use]
pub fn asset_bytes(doc: &Document) -> u64 {
    doc.assets.values().map(|a| a.data.len() as u64).sum()
}

/// Objects on all pages.
#[must_use]
pub fn object_count(doc: &Document) -> usize {
    doc.pages.iter().map(|p| p.objects.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_spec_makes_the_same_board_with_the_requested_size_and_variety() {
        let spec = BoardSpec::sized(1_000, 7);
        let a = seed_document(&spec);
        assert_eq!(object_count(&a), 1_000);
        assert_eq!(a.pages.len(), 2);
        assert_eq!(serde_json::to_string(&a.pages).unwrap(), serde_json::to_string(&seed_document(&spec).pages).unwrap());
        let kinds = |f: fn(&Kind) -> bool| a.pages.iter().flat_map(|p| &p.objects).filter(|o| f(&o.kind)).count();
        assert!(kinds(|k| matches!(k, Kind::Box(_))) > 200);
        assert!(kinds(|k| matches!(k, Kind::Connector(_))) > 20);
        assert!(kinds(|k| matches!(k, Kind::Pen(_))) > 20);
        assert!(kinds(|k| matches!(k, Kind::Table(_))) > 10);
        assert!(kinds(|k| matches!(k, Kind::Image(_))) > 50);
        assert!(a.pages.iter().flat_map(|p| &p.objects).any(|o| matches!(&o.kind, Kind::Box(b) if b.lang.is_some())));
        // every object passes the host's own validation (what a guest could submit)
        let l = inkboard_session::Limits::host_trust();
        for p in &a.pages {
            let ids: std::collections::HashSet<_> = p.objects.iter().map(|o| &o.id).collect();
            assert_eq!(ids.len(), p.objects.len(), "identifiers are unique");
            for o in &p.objects {
                inkboard_session::limits::check_object(&l, o).unwrap_or_else(|e| panic!("{e}: {o:?}"));
            }
        }
        assert!(asset_bytes(&a) > 5_000_000);
    }

    #[test]
    fn the_v4_fields_are_added_to_a_share_of_the_objects_and_stay_valid() {
        let mut doc = seed_document(&BoardSpec::sized(1_000, 7));
        let before = serde_json::to_string(&doc.pages).unwrap();
        add_v4_fields(&mut doc, 7);
        let all = || doc.pages.iter().flat_map(|p| p.objects.iter());
        assert!(all().any(|o| o.style.shadow));
        assert!(all().any(|o| matches!(&o.kind, Kind::Connector(c) if c.has_custom_head_scale())));
        assert!(all().any(|o| matches!(&o.kind, Kind::Image(i) if !i.recolor.is_empty())));
        assert_ne!(before, serde_json::to_string(&doc.pages).unwrap());
        let l = inkboard_session::Limits::host_trust();
        for o in all() {
            inkboard_session::limits::check_object(&l, o).unwrap_or_else(|e| panic!("{e}: {o:?}"));
        }
    }
}
