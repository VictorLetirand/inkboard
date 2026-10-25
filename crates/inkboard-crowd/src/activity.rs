//! What a simulated person does: moves the cursor, draws strokes, creates, moves and deletes objects, edits texts, reorders, undoes and
//! pastes images, at the rates of the `Profile`. Everything goes through the real `Client`, so it is validated and ordered like a
//! real guest's work.

use crate::board::rich_text;
use crate::guest::{SimGuest, UPLOAD_CHUNK, UPLOAD_CHUNKS_PER_SEC, UPLOAD_WINDOW, Upload, marker};
use crate::png::noise_png_of_size;
use crate::rng::Rng;
use inkboard_core::Id;
use inkboard_core::connector::{Arrowhead, Connector, End, Route};
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{BoxGeom, ImageObj, Kind, Label, Object};
use inkboard_core::style::{Align, FILLS, PALETTE, Style};
use inkboard_session::WireOp;
use inkboard_session::assets::hash_hex;
use inkboard_session::proto::{AssetChunk, Message, Presence, UploadStatus};
use std::sync::Arc;

use crate::profile::Profile;

const CURSOR_PHASE_MS: f64 = 4_000.0;
const GESTURE_EVERY_MS: u64 = 50;
const AREA_RADIUS: f64 = 1_400.0;

enum Gesture {
    Stroke { id: Id, page: Id, pts: Vec<Pt>, sent: usize, end: u64, last: u64, dir: (f64, f64) },
    Drag { page: Id, orig: Box<Object>, to: Pt, start: u64, end: u64, last: u64 },
    Typing { page: Id, obj: Box<Object>, end: u64 },
    Upload,
}

const ACTIONS: usize = 8;
const STROKE: usize = 0;
const CREATE: usize = 1;
const MOVE: usize = 2;
const DELETE: usize = 3;
const TEXT: usize = 4;
const REORDER: usize = 5;
const UNDO: usize = 6;
const IMAGE: usize = 7;

pub(crate) struct State {
    ready: bool,
    page: usize,
    area: Pt,
    area_until: u64,
    cursor: Pt,
    moving: bool,
    phase_until: u64,
    next_cursor: u64,
    next: [u64; ACTIONS],
    gesture: Option<Gesture>,
    mine: Vec<(Id, Id)>,
    edits_since_undo: u32,
    counter: u64,
    selection: Vec<Id>,
    next_drop: u64,
    pinned: bool,
    fixed_area: Option<Pt>,
}

impl State {
    pub fn new(rng: &mut Rng, _p: &Profile) -> Self {
        Self {
            ready: false,
            page: rng.below(8),
            area: Pt::ZERO,
            area_until: 0,
            cursor: Pt::ZERO,
            moving: false,
            phase_until: 0,
            next_cursor: 0,
            next: [0; ACTIONS],
            gesture: None,
            mine: Vec::new(),
            edits_since_undo: 0,
            counter: 0,
            selection: Vec::new(),
            next_drop: u64::MAX,
            pinned: false,
            fixed_area: None,
        }
    }

    pub fn pin_area(&mut self, at: Pt) {
        self.fixed_area = Some(at);
    }

    pub fn pin_page(&mut self, page: usize) {
        self.page = page;
        self.pinned = true;
    }

    /// The connection was cut: whatever gesture was in progress is lost with it.
    pub fn on_disconnect(&mut self) {
        self.gesture = None;
    }
}

fn mean_ms(per_min: f64) -> f64 {
    if per_min <= 0.0 { f64::INFINITY } else { 60_000.0 / per_min }
}

fn anchor_of(o: &Object) -> Pt {
    match &o.kind {
        Kind::Box(b) => b.frame.center(),
        Kind::Image(i) => i.frame.center(),
        Kind::Pen(p) => p.pts.first().copied().unwrap_or(Pt::ZERO),
        Kind::Table(t) => t.origin,
        Kind::Connector(c) => match &c.start {
            End::Free { at } => *at,
            End::Bound { .. } => Pt::ZERO,
        },
    }
}

fn init(g: &mut SimGuest, now: u64) {
    g.act.ready = true;
    let pages = g.client.doc().pages.len().max(1);
    g.act.page %= pages;
    pick_area(g, now);
    g.act.cursor = g.act.area;
    let p = g.env.profile.clone();
    let rates = [p.stroke_per_min, p.create_per_min, p.move_per_min, p.delete_per_min, p.text_per_min, p.reorder_per_min, p.undo_per_min, p.image_per_min];
    for (i, r) in rates.iter().enumerate() {
        let m = mean_ms(*r);
        g.act.next[i] = if m.is_finite() { now + g.rng.exp(m) as u64 } else { u64::MAX };
    }
    g.act.moving = g.rng.chance(p.cursor_active);
    g.act.phase_until = now + g.rng.exp(CURSOR_PHASE_MS) as u64;
    g.act.next_cursor = now;
    if p.drops_per_hour > 0.0 {
        g.act.next_drop = now + g.rng.exp(3_600_000.0 / p.drops_per_hour) as u64;
    }
}

/// A new place to work: around a random object of a random page.
fn pick_area(g: &mut SimGuest, now: u64) {
    let doc = g.client.doc();
    let pages = doc.pages.len().max(1);
    if !g.act.pinned && g.rng.chance(0.2) {
        g.act.page = g.rng.below(pages);
    }
    let page = &doc.pages[g.act.page % pages];
    g.act.area = if let Some(at) = g.act.fixed_area {
        at
    } else if page.objects.is_empty() {
        Pt::new(400.0, 300.0)
    } else {
        anchor_of(&page.objects[g.rng.below(page.objects.len())])
    };
    g.act.area_until = now + 15_000 + g.rng.below(30_000) as u64;
}

fn page_id(g: &SimGuest) -> Option<Id> {
    let pages = &g.client.doc().pages;
    pages.get(g.act.page % pages.len().max(1)).map(|p| p.id.clone())
}

/// An object of the current page, preferably near the work area (`mine`: preferably one made by this guest).
fn pick_object(g: &mut SimGuest, mine_first: bool, want: impl Fn(&Object) -> bool) -> Option<(Id, Object)> {
    let page_i = g.act.page % g.client.doc().pages.len().max(1);
    if mine_first && g.rng.chance(0.6) {
        for _ in 0..4 {
            if g.act.mine.is_empty() {
                break;
            }
            let (pid, oid) = g.act.mine[g.rng.below(g.act.mine.len())].clone();
            if let Some(o) = g.client.doc().page(&pid).and_then(|p| p.get(&oid)).filter(|o| want(o)) {
                return Some((pid, o.clone()));
            }
        }
    }
    let area = g.act.area;
    let page = &g.client.doc().pages[page_i];
    if page.objects.is_empty() {
        return None;
    }
    let n = page.objects.len();
    let mut first: Option<&Object> = None;
    for _ in 0..16 {
        let o = &page.objects[g.rng.below(n)];
        if !want(o) || page.is_locked(o) {
            continue;
        }
        if anchor_of(o).dist(area) < AREA_RADIUS {
            return Some((page.id.clone(), o.clone()));
        }
        first.get_or_insert(o);
    }
    first.map(|o| (page.id.clone(), o.clone()))
}

fn style(rng: &mut Rng) -> Style {
    Style { stroke: *rng.pick(&PALETTE), fill: rng.chance(0.6).then(|| *rng.pick(&FILLS)), width: *rng.pick(&[2.0, 3.0]), ..Style::default() }
}

fn sentence(rng: &mut Rng, words: usize) -> String {
    const W: &[&str] = &["idea", "todo", "check", "later", "yes", "no", "why", "ship it", "risk", "owner", "next", "done", "question", "agree"];
    (0..words).map(|_| *rng.pick(W)).collect::<Vec<_>>().join(" ")
}

fn new_id(g: &mut SimGuest) -> Id {
    g.act.counter += 1;
    marker(g.env.machine, g.plan.index, g.act.counter, g.env.clock.epoch_us())
}

fn presence(g: &mut SimGuest, editing: Option<Id>) {
    let Some(page) = page_id(g) else { return };
    let tool = if g.rng.chance(0.2) { format!("L{}:{:x}", g.env.machine, g.env.clock.epoch_us()) } else { "select".to_owned() };
    let p = Presence {
        participant: None,
        page,
        cursor: Some(g.act.cursor),
        selection: g.act.selection.clone(),
        tool: Some(tool),
        editing,
        viewport: Some(SimGuest::viewport(g.act.area)),
    };
    g.presence(p);
}

/// One turn of the person. Returns `true` when something was sent.
pub(crate) fn act(g: &mut SimGuest, now: u64) -> bool {
    if g.rep.snapshot_ms.is_none() {
        return false; // nothing to work on before the snapshot arrived
    }
    if !g.act.ready {
        init(g, now);
    }
    let mut busy = cursor(g, now);
    busy |= gesture_tick(g, now);
    if now >= g.act.area_until && g.act.gesture.is_none() {
        pick_area(g, now);
    }
    if now >= g.act.next_drop && !g.quiet {
        let mean = 3_600_000.0 / g.env.profile.drops_per_hour.max(1e-9);
        g.act.next_drop = now + g.rng.exp(mean) as u64;
        let (lo, hi) = g.env.profile.drop_pause_s;
        let pause = (g.rng.range(lo, hi) * 1000.0) as u64;
        g.drop_connection(now, pause);
        return true;
    }
    if g.quiet || g.client.role() != inkboard_session::Role::Edit || g.act.gesture.is_some() {
        return busy;
    }
    for a in 0..ACTIONS {
        if now < g.act.next[a] {
            continue;
        }
        let p = &g.env.profile;
        let rates = [p.stroke_per_min, p.create_per_min, p.move_per_min, p.delete_per_min, p.text_per_min, p.reorder_per_min, p.undo_per_min, p.image_per_min];
        g.act.next[a] = now + g.rng.exp(mean_ms(rates[a])).min(1e12) as u64;
        busy |= match a {
            STROKE => start_stroke(g, now),
            CREATE => create(g, now),
            MOVE => start_drag(g, now),
            DELETE => delete(g, now),
            TEXT => start_typing(g, now),
            REORDER => reorder(g, now),
            UNDO => undo(g),
            IMAGE => start_upload(g, now),
            _ => false,
        };
        if g.act.gesture.is_some() {
            break;
        }
    }
    busy
}

fn cursor(g: &mut SimGuest, now: u64) -> bool {
    let p = &g.env.profile;
    let (active, hz) = (p.cursor_active, p.cursor_hz);
    if now >= g.act.phase_until {
        g.act.moving = if active >= 1.0 {
            true
        } else if active <= 0.0 {
            false
        } else {
            !g.act.moving
        };
        let mean = if g.act.moving { CURSOR_PHASE_MS } else { CURSOR_PHASE_MS * (1.0 - active) / active.max(1e-9) };
        g.act.phase_until = now + g.rng.exp(mean.max(200.0)) as u64;
    }
    if !g.act.moving || hz <= 0.0 || now < g.act.next_cursor {
        return false;
    }
    g.act.next_cursor = now + (1000.0 / hz) as u64;
    if g.act.gesture.is_none() {
        // wander around the work area
        let to = g.act.area;
        let c = g.act.cursor;
        let (dx, dy) = (g.rng.range(-40.0, 40.0), g.rng.range(-30.0, 30.0));
        let pull = 0.02;
        g.act.cursor = Pt::new(c.x + dx + (to.x - c.x) * pull, c.y + dy + (to.y - c.y) * pull);
    }
    let editing = match &g.act.gesture {
        Some(Gesture::Typing { obj, .. }) => Some(obj.id.clone()),
        _ => None,
    };
    presence(g, editing);
    true
}

fn gesture_tick(g: &mut SimGuest, now: u64) -> bool {
    let Some(gesture) = g.act.gesture.take() else { return false };
    match gesture {
        Gesture::Stroke { id, page, mut pts, mut sent, end, mut last, mut dir } => {
            if now.saturating_sub(last) >= GESTURE_EVERY_MS && now < end {
                last = now;
                let mut cur = *pts.last().unwrap_or(&g.act.cursor);
                for _ in 0..3 {
                    dir = ((dir.0 + g.rng.range(-0.7, 0.7)).clamp(-5.0, 5.0), (dir.1 + g.rng.range(-0.7, 0.7)).clamp(-5.0, 5.0));
                    cur = Pt::new(cur.x + dir.0, cur.y + dir.1);
                    pts.push(cur);
                }
                g.act.cursor = cur;
                let batch = pts[sent..].to_vec();
                sent = pts.len();
                g.rep.stroke_batches_sent += 1;
                g.ephemeral(Message::StrokePoints { stroke: id.clone(), points: batch, author: None });
                g.act.gesture = Some(Gesture::Stroke { id, page, pts, sent, end, last, dir });
                return true;
            }
            if now >= end {
                let mut o = Object::new_pen(pts, false, Style { fill: None, width: 3.0, ..style(&mut g.rng) });
                o.id = id.clone();
                g.act.mine.push((page.clone(), id));
                send_put(g, page, o, now);
                return true;
            }
            g.act.gesture = Some(Gesture::Stroke { id, page, pts, sent, end, last, dir });
            false
        }
        Gesture::Drag { page, orig, to, start, end, mut last } => {
            if now >= end {
                let mut o = (*orig).clone();
                o.translate(to);
                send_put(g, page, o, now);
                return true;
            }
            if now.saturating_sub(last) >= GESTURE_EVERY_MS {
                last = now;
                let f = (now - start) as f64 / (end - start).max(1) as f64;
                let mut state = (*orig).clone();
                state.translate(Pt::new(to.x * f, to.y * f));
                g.act.cursor = anchor_of(&state);
                g.rep.transients_sent += 1;
                g.ephemeral(Message::Transient { object: state.id.clone(), state: Box::new(state), author: None });
                g.act.gesture = Some(Gesture::Drag { page, orig, to, start, end, last });
                return true;
            }
            g.act.gesture = Some(Gesture::Drag { page, orig, to, start, end, last });
            false
        }
        Gesture::Typing { page, obj, end } => {
            if now >= end {
                send_put(g, page, *obj, now);
                return true;
            }
            g.act.gesture = Some(Gesture::Typing { page, obj, end });
            false
        }
        Gesture::Upload => upload_tick(g, now),
    }
}

fn send_put(g: &mut SimGuest, page: Id, o: Object, now: u64) {
    g.act.selection = vec![o.id.clone()];
    if g.edit(vec![WireOp::Put { page, object: Box::new(o) }], now).is_some() {
        g.act.edits_since_undo += 1;
    }
}

fn start_stroke(g: &mut SimGuest, now: u64) -> bool {
    let Some(page) = page_id(g) else { return false };
    // not a latency marker: the object only exists when the stroke is finished, seconds after this identifier was made
    g.act.counter += 1;
    let id = Id(format!("k{}-{}-{}", g.env.machine, g.plan.index, g.act.counter));
    let start = Pt::new(g.act.area.x + g.rng.range(-600.0, 600.0), g.act.area.y + g.rng.range(-400.0, 400.0));
    let dur = g.rng.range(1_000.0, 4_000.0) as u64;
    g.act.cursor = start;
    g.act.gesture =
        Some(Gesture::Stroke { id, page, pts: vec![start], sent: 0, end: now + dur, last: now, dir: (g.rng.range(-3.0, 3.0), g.rng.range(-3.0, 3.0)) });
    true
}

fn create(g: &mut SimGuest, now: u64) -> bool {
    let Some(page) = page_id(g) else { return false };
    let at = Pt::new(g.act.area.x + g.rng.range(-700.0, 700.0), g.act.area.y + g.rng.range(-450.0, 450.0));
    let roll = g.rng.f64();
    let id = new_id(g);
    let mut o = if roll < 0.62 {
        let geom = *g.rng.pick(&[BoxGeom::Rect, BoxGeom::Ellipse, BoxGeom::Diamond, BoxGeom::Sticky, BoxGeom::Sticky]);
        let words = 1 + g.rng.below(5);
        let label = Label::new(&sentence(&mut g.rng, words), 18.0, Align::Center);
        Object::new_box(geom, Frame::new(at.x, at.y, 120.0 + g.rng.range(0.0, 100.0), 70.0 + g.rng.range(0.0, 50.0)), style(&mut g.rng), label)
    } else if roll < 0.77 {
        let words = 6 + g.rng.below(30);
        let label = Label { text: rich_text(&mut g.rng, words), size: 16.0, align: Align::Left, ..Label::default() };
        Object::new_box(BoxGeom::Text, Frame::new(at.x, at.y, 280.0, 40.0 + words as f64 * 2.0), Style { fill: None, width: 0.0, ..style(&mut g.rng) }, label)
    } else if roll < 0.9 {
        // a connector between two shapes
        let a = pick_object(g, true, |o| matches!(&o.kind, Kind::Box(b) if b.geom != BoxGeom::Text));
        let b = pick_object(g, true, |o| matches!(&o.kind, Kind::Box(b) if b.geom != BoxGeom::Text));
        let (Some((_, a)), Some((_, b))) = (a, b) else { return false };
        if a.id == b.id {
            return false;
        }
        let mut c = Connector::new(End::bound(a.id), End::bound(b.id));
        c.route = *g.rng.pick(&[Route::Straight, Route::Elbow]);
        c.head_end = Arrowhead::Arrow;
        Object::new(Kind::Connector(c), Style { width: 2.0, fill: None, ..style(&mut g.rng) })
    } else {
        // an icon from the board's library (any small vector image the host announced)
        let icon = g.manifest_icon();
        let Some(asset) = icon else { return false };
        let s = 40.0 + g.rng.range(0.0, 50.0);
        Object::new(
            Kind::Image(ImageObj { frame: Frame::new(at.x, at.y, s, s), asset, latex: None, mono: false }),
            Style { width: 0.0, fill: None, ..Style::default() },
        )
    };
    o.id = id.clone();
    g.act.mine.push((page.clone(), id));
    if g.act.mine.len() > 300 {
        g.act.mine.remove(0);
    }
    send_put(g, page, o, now);
    true
}

fn start_drag(g: &mut SimGuest, now: u64) -> bool {
    let Some((page, o)) = pick_object(g, true, |o| !matches!(o.kind, Kind::Connector(_))) else { return false };
    let to = Pt::new(g.rng.range(-400.0, 400.0), g.rng.range(-300.0, 300.0));
    let dur = g.rng.range(600.0, 2_000.0) as u64;
    g.act.selection = vec![o.id.clone()];
    g.act.gesture = Some(Gesture::Drag { page, orig: Box::new(o), to, start: now, end: now + dur, last: now });
    true
}

fn delete(g: &mut SimGuest, now: u64) -> bool {
    let Some((page, o)) = pick_object(g, true, |_| true) else { return false };
    g.act.mine.retain(|(_, id)| *id != o.id);
    if g.edit(vec![WireOp::Delete { page, ids: vec![o.id] }], now).is_some() {
        g.act.edits_since_undo += 1;
    }
    true
}

fn start_typing(g: &mut SimGuest, now: u64) -> bool {
    let Some((page, mut o)) = pick_object(g, true, |o| matches!(o.kind, Kind::Box(_))) else { return false };
    let words = 3 + g.rng.below(25);
    let text = rich_text(&mut g.rng, words);
    if let Kind::Box(b) = &mut o.kind {
        b.label.text = text;
    }
    g.act.selection = vec![o.id.clone()];
    let dur = g.rng.range(1_500.0, 5_000.0) as u64;
    g.act.gesture = Some(Gesture::Typing { page, obj: Box::new(o), end: now + dur });
    true
}

fn reorder(g: &mut SimGuest, now: u64) -> bool {
    let Some((page, o)) = pick_object(g, true, |_| true) else { return false };
    let Some(p) = g.client.doc().page(&page) else { return false };
    let mut ids: Vec<Id> = p.objects.iter().map(|x| x.id.clone()).filter(|i| *i != o.id).collect();
    ids.push(o.id);
    if g.edit(vec![WireOp::Order { page, ids }], now).is_some() {
        g.act.edits_since_undo += 1;
    }
    true
}

fn undo(g: &mut SimGuest) -> bool {
    if g.act.edits_since_undo == 0 {
        return false;
    }
    g.act.edits_since_undo -= 1;
    if g.client.request_undo(false) {
        g.rep.undos_sent += 1;
        return true;
    }
    false
}

fn start_upload(g: &mut SimGuest, now: u64) -> bool {
    let Some(page) = page_id(g) else { return false };
    let p = &g.env.profile;
    let big = g.rng.chance(p.big_image_prob);
    let bytes = if big { g.rng.range(2.0, 8.0) * 1_048_576.0 } else { g.rng.range(30.0, 400.0) * 1024.0 } as usize;
    let data = noise_png_of_size(bytes, g.rng.next_u64());
    let hash = hash_hex(&data);
    g.known_images.insert(hash.clone());
    let at = Pt::new(g.act.area.x + g.rng.range(-500.0, 500.0), g.act.area.y + g.rng.range(-300.0, 300.0));
    g.upload = Some(Upload {
        hash,
        data: Arc::new(data),
        next: 0,
        acked: 0,
        last_ack_ms: now,
        stored: false,
        page,
        frame_at: at,
        chunks_this_second: (now / 1000, 0),
    });
    g.act.gesture = Some(Gesture::Upload);
    true
}

fn upload_tick(g: &mut SimGuest, now: u64) -> bool {
    let Some(mut up) = g.upload.take() else { return false };
    let size = up.data.len() as u64;
    if up.stored {
        let (w, h) = (240.0, 180.0);
        let mut o = Object::new(
            Kind::Image(ImageObj { frame: Frame::new(up.frame_at.x, up.frame_at.y, w, h), asset: Id(up.hash.clone()), latex: None, mono: false }),
            Style { width: 0.0, fill: None, ..Style::default() },
        );
        o.id = new_id(g);
        g.rep.uploads_done += 1;
        send_put(g, up.page.clone(), o, now);
        return true;
    }
    let mut busy = false;
    if up.next > up.acked && now.saturating_sub(up.last_ack_ms) > 2_000 {
        up.next = up.acked; // nothing heard for a while: go on from what the host holds
        up.last_ack_ms = now;
    }
    if up.chunks_this_second.0 != now / 1000 {
        up.chunks_this_second = (now / 1000, 0);
    }
    while up.next < size && up.next - up.acked < UPLOAD_WINDOW && up.chunks_this_second.1 < UPLOAD_CHUNKS_PER_SEC {
        let from = up.next as usize;
        let to = (from + UPLOAD_CHUNK).min(size as usize);
        g.raw_chunk(AssetChunk { hash: up.hash.clone(), offset: up.next, data: up.data[from..to].to_vec(), last: to as u64 == size });
        up.next = to as u64;
        up.chunks_this_second.1 += 1;
        busy = true;
    }
    g.upload = Some(up);
    g.act.gesture = Some(Gesture::Upload);
    busy
}

/// The host acknowledged a piece of an upload.
pub(crate) fn on_ack(g: &mut SimGuest, hash: &str, have: u64, status: &UploadStatus, now: u64) {
    let Some(up) = g.upload.as_mut().filter(|u| u.hash == hash) else { return };
    up.last_ack_ms = now;
    match status {
        UploadStatus::Receiving => up.acked = up.acked.max(have),
        UploadStatus::Stored => up.stored = true,
        UploadStatus::Refused { .. } => {
            g.upload = None;
            g.act.gesture = None;
            g.rep.uploads_refused += 1;
        }
    }
}
