//! Presence of participants on the canvas: colored cursor with a name badge, short trail, halo on touched objects, selection outline.
//!
//! First user: the MCP agent acting in the application (see `live.rs`); the same pieces will draw remote humans in a shared session.
//! Everything here is cosmetic: it never touches the document, its history or its revision, nor what the MCP tools answer.
//! Motion is a pure function of time (`PresenceState::position_at`), so drawing needs no per-frame state update.

use crate::app::App;
use crate::i18n::{t, tr};
use crate::platform::Instant;
use crate::render::Cam;
use eframe::egui::epaint::Mesh;
use eframe::egui::{self, Color32, FontId, Painter, Pos2, Shape, Stroke, StrokeKind, vec2};
use inkboard_core::geom::{Pt, Rect};
use inkboard_core::{Id, Page};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::Duration;

/// A cursor disappears after this much inactivity (it fades during the last `FADE`).
pub const IDLE: Duration = Duration::from_secs(5);
const FADE: Duration = Duration::from_secs(1);
/// Lifetime of the halo on a touched object.
pub const HALO_LIFE: Duration = Duration::from_millis(1500);
const TRAIL_LIFE: Duration = Duration::from_millis(400);
const TRAIL_SAMPLES: u32 = 10;
/// Lifetime of the discreet pulse of a "look" (reading tools).
const LOOK_LIFE: Duration = Duration::from_millis(900);
/// A batch is shown as at most this many stops (the targets of the others are merged into them).
const MAX_STOPS: usize = 24;
/// Events kept for the session host (lot C) to drain; the oldest are dropped.
const OUTBOX_MAX: usize = 256;
/// Time a remote human cursor takes to reach its newest position.
const GLIDE: Duration = Duration::from_millis(90);
/// A stroke being drawn by somebody else disappears after this long without a new point (its final form arrives as an object).
const STROKE_IDLE: Duration = Duration::from_secs(3);
/// Points kept of a stroke somebody else is drawing.
const MAX_STROKE_POINTS: usize = 20_000;
/// The "is typing" badge disappears when no presence message renews it (a message comes at least every 2 s).
const EDITING_IDLE: Duration = Duration::from_secs(6);
/// Tools that only read the board: shown as a look, never as a move.
const LOOK_TOOLS: [&str; 7] = ["describe_board", "board_changes", "render", "export", "find_objects", "search_icons", "list_icon_groups"];

// Human participants and the identity fields below are used by the session lots (remote cursors, forwarded agent events).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticipantKind {
    Human,
    Agent,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub color: Color32,
    pub kind: ParticipantKind,
}

impl Participant {
    /// The MCP agent of this application.
    pub fn agent(name: &str) -> Self {
        Self { id: "agent".into(), name: name.into(), color: Color32::from_rgb(0xd9, 0x77, 0x57), kind: ParticipantKind::Agent }
    }
}

/// An object touched by an agent call, with its frame (world units) after the call.
#[derive(Clone, Debug)]
pub struct Target {
    pub id: Id,
    pub frame: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Reading tool: the agent looks, nothing moves.
    Look,
    /// Tool that may modify the board.
    Act,
}

/// One agent tool call, as seen by the presence layer.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AgentEvent {
    pub name: String,
    pub tool: String,
    pub action: Action,
    /// Objects touched, one group per stop of the cursor (a `batch` gives one group per operation).
    pub steps: Vec<Vec<Target>>,
    pub at: Instant,
}

impl AgentEvent {
    #[allow(dead_code)]
    pub fn targets(&self) -> impl Iterator<Item = &Target> {
        self.steps.iter().flatten()
    }
}

#[derive(Clone, Debug)]
struct Leg {
    from: Pt,
    to: Pt,
    start: Instant,
    end: Instant,
}

#[derive(Clone, Debug)]
struct Halo {
    frame: Rect,
    start: Instant,
}

#[derive(Clone, Debug)]
pub struct PresenceState {
    pub participant: Participant,
    /// Where the cursor rests (or is going), world units.
    pub cursor: Option<Pt>,
    /// Frames (world units) of the objects the participant has selected, resolved by whoever owns the page.
    pub selection: Vec<Rect>,
    pub last_activity: Option<Instant>,
    legs: Vec<Leg>,
    halos: Vec<Halo>,
    look: Option<Instant>,
    /// Screenshot demos: time is held still at this instant.
    frozen: Option<Instant>,
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl PresenceState {
    pub fn new(participant: Participant) -> Self {
        Self { participant, cursor: None, selection: Vec::new(), last_activity: None, legs: Vec::new(), halos: Vec::new(), look: None, frozen: None }
    }

    /// Forgets everything shown (e.g. the animation setting is turned off).
    pub fn clear(&mut self) {
        *self = Self::new(self.participant.clone());
    }

    /// Holds time still at `at` (screenshot demos).
    pub fn freeze(&mut self, at: Instant) {
        self.frozen = Some(at);
    }

    fn clock(&self, now: Instant) -> Instant {
        self.frozen.unwrap_or(now)
    }

    #[cfg(test)]
    pub fn halo_count(&self) -> usize {
        self.halos.len()
    }

    /// Instant at which the last queued move ends.
    pub fn settled_at(&self) -> Option<Instant> {
        self.legs.last().map(|l| l.end)
    }

    /// Cursor position (world units) at `now`: eased along the queued legs.
    pub fn position_at(&self, now: Instant) -> Option<Pt> {
        for l in &self.legs {
            if now < l.start {
                return Some(l.from);
            }
            if now < l.end {
                let t = now.duration_since(l.start).as_secs_f64() / l.end.duration_since(l.start).as_secs_f64().max(1e-6);
                return Some(l.from.lerp(l.to, smooth(t)));
            }
        }
        self.cursor
    }

    /// 1 while active, fading to 0 at the end of the idle delay.
    pub fn opacity(&self, now: Instant) -> f32 {
        let Some(last) = self.last_activity else { return 0.0 };
        let idle = now.saturating_duration_since(last);
        if idle >= IDLE {
            0.0
        } else if idle + FADE > IDLE {
            (IDLE - idle).as_secs_f32() / FADE.as_secs_f32()
        } else {
            1.0
        }
    }

    /// Does drawing at `now` still change from frame to frame (the caller then asks for a repaint)?
    pub fn is_animating(&self, now: Instant) -> bool {
        let now = self.clock(now);
        self.frozen.is_none() && self.opacity(now) > 0.0
    }

    /// A human participant's cursor moved (about 20 updates a second): glides there in a few frames instead of jumping, and counts as
    /// activity. Unlike an agent's queued legs, a new position replaces the move still in progress.
    pub fn glide_to(&mut self, to: Pt, now: Instant) {
        let from = self.position_at(now).unwrap_or(to);
        self.legs.retain(|l| l.end <= now && now.saturating_duration_since(l.end) < TRAIL_LIFE * 2);
        if from.dist(to) > 0.01 {
            self.legs.push(Leg { from, to, start: now, end: now + GLIDE });
        }
        // a keep-alive repeating the same position is not activity: a resting cursor fades after the idle delay
        if self.cursor != Some(to) || self.last_activity.is_none() {
            self.last_activity = Some(now);
        }
        self.cursor = Some(to);
    }

    fn push_leg(&mut self, to: Pt, anchor: Pt, now: Instant, fast: bool) -> Instant {
        self.legs.retain(|l| now.saturating_duration_since(l.end) < TRAIL_LIFE * 2);
        let from = self.cursor.unwrap_or(anchor);
        let start = self.legs.last().map_or(now, |l| l.end.max(now));
        let d = from.dist(to);
        let secs = if fast { (0.12 + d / 6000.0).min(0.3) } else { (0.25 + d / 3000.0).min(0.7) };
        let end = start + Duration::from_secs_f64(secs);
        self.legs.push(Leg { from, to, start, end });
        self.cursor = Some(to);
        end
    }

    /// Applies an agent event. `anchor`: where the cursor first appears (the center of the view).
    pub fn act(&mut self, ev: &AgentEvent, anchor: Pt, now: Instant) {
        self.halos.retain(|h| now.saturating_duration_since(h.start) < HALO_LIFE);
        let mut settled = now;
        if ev.action == Action::Act && !ev.steps.is_empty() {
            let fast = ev.steps.len() > 1;
            for stop in ev.steps.chunks(ev.steps.len().div_ceil(MAX_STOPS)) {
                let frames: Vec<Rect> = stop.iter().flatten().map(|t| t.frame).collect();
                let Some(union) = frames.iter().copied().reduce(|a, b| a.union(&b)) else { continue };
                let arrival = self.push_leg(union.center(), anchor, now, fast);
                self.halos.extend(frames.into_iter().map(|frame| Halo { frame, start: arrival }));
                settled = arrival;
            }
        } else {
            // a look (or a call without a visible target): the cursor stays where it is and pulses
            self.cursor.get_or_insert(anchor);
            self.look = Some(now);
        }
        self.last_activity = Some(settled);
    }
}

/* ------------------------------------------------------------------ the agent */

/// The agent's presence in the application: the setting, what is shown, and the events waiting for the session host.
pub struct AgentPresence {
    /// Setting "Animate the agent".
    pub animate: bool,
    /// No visible window (screenshots): events are recorded but nothing is animated.
    pub headless: bool,
    pub state: PresenceState,
    pub outbox: VecDeque<AgentEvent>,
    /// The other people of a shared session (a guest sees the host and the other guests).
    pub remote: RemotePresence,
}

/// A participant of a shared session other than this user, with the page they are looking at.
#[derive(Clone, Debug)]
pub struct RemotePerson {
    pub page: Id,
    pub state: PresenceState,
    /// Object whose text they are editing, and when that was last said (the badge fades when the news stops coming).
    pub editing: Option<(Id, Instant)>,
    /// Area of the page they see (what "Follow" follows).
    pub viewport: Option<Rect>,
}

impl RemotePerson {
    pub fn new(page: Id, state: PresenceState) -> Self {
        Self { page, state, editing: None, viewport: None }
    }
}

/// A stroke somebody else is drawing right now (points received so far, in world units).
#[derive(Clone, Debug)]
pub struct RemoteStroke {
    pub author: u32,
    pub pts: Vec<Pt>,
    pub last: Instant,
}

/// An object somebody else is moving or resizing right now, as they show it (drawn over the board, never part of it).
#[derive(Clone, Debug)]
pub struct RemoteGhost {
    pub author: u32,
    pub object: inkboard_core::Object,
    pub last: Instant,
}

/// Remote participants shown on the canvas: cursor with name, selection outline, stroke in progress. Cosmetic like the rest.
#[derive(Clone, Debug, Default)]
pub struct RemotePresence {
    pub people: std::collections::BTreeMap<u32, RemotePerson>,
    pub strokes: std::collections::BTreeMap<Id, RemoteStroke>,
    /// Objects being moved or resized by somebody else, on the host (a guest's mirror shows them in place instead).
    pub ghosts: std::collections::BTreeMap<Id, RemoteGhost>,
    /// Page shown to this user: only the people on that page are drawn.
    pub page: Option<Id>,
    /// The participant whose view this user's view follows.
    pub following: Option<u32>,
    /// Page and view set by the last following step: any other value means the user moved the view.
    follow_set: Option<(Id, inkboard_core::View)>,
    /// Added to the clock by [`Self::now`]: tests make time pass with it instead of waiting.
    pub skew: Duration,
}

impl RemotePresence {
    /// Forgets everything (the session is over).
    pub fn clear(&mut self) {
        *self = Self { skew: self.skew, ..Self::default() };
    }

    /// The instant presence is timed with.
    pub fn now(&self) -> Instant {
        Instant::now() + self.skew
    }

    /// Is something drawn that changes with time (the caller then asks for a repaint)?
    pub fn is_animating(&self, now: Instant) -> bool {
        self.people.values().any(|p| p.state.is_animating(now) || p.editing.is_some()) || !self.strokes.is_empty() || !self.ghosts.is_empty()
    }

    /// New points of a stroke in progress.
    pub fn add_stroke(&mut self, stroke: Id, author: u32, points: Vec<Pt>, now: Instant) {
        let s = self.strokes.entry(stroke).or_insert_with(|| RemoteStroke { author, pts: Vec::new(), last: now });
        if s.pts.len() < MAX_STROKE_POINTS {
            s.pts.extend(points);
        }
        s.last = now;
    }

    /// The gesture of `author` is over (its result arrived as an ordinary modification) or cannot continue: nothing of it remains drawn.
    pub fn settle(&mut self, author: u32) {
        self.strokes.retain(|_, s| s.author != author);
        self.ghosts.retain(|_, g| g.author != author);
    }

    /// `author` is still there (a presence message arrived): a gesture held still is not a gesture abandoned.
    pub fn touch(&mut self, author: u32, now: Instant) {
        self.strokes.values_mut().filter(|s| s.author == author).for_each(|s| s.last = now);
        self.ghosts.values_mut().filter(|g| g.author == author).for_each(|g| g.last = now);
    }

    fn paint(&self, painter: &Painter, cam: &Cam, now: Instant, page: &Page) {
        let here = |p: &RemotePerson| self.page.as_ref().is_none_or(|cur| *cur == p.page);
        for p in self.people.values().filter(|p| here(p)) {
            draw_presence(painter, cam, &p.state, now);
            if let Some((id, _)) = p.editing.as_ref().filter(|(_, seen)| now.saturating_duration_since(*seen) < EDITING_IDLE)
                && let Some(o) = page.get(id)
            {
                draw_editing_badge(painter, cam.rect_s(page.bounds(o)), &p.state.participant);
            }
        }
        for s in self.strokes.values() {
            let owner = self.people.get(&s.author);
            if owner.is_some_and(|p| !here(p)) {
                continue;
            }
            let color = owner.map_or(Color32::GRAY, |p| p.state.participant.color);
            let pts: Vec<Pos2> = s.pts.iter().map(|p| cam.s(*p)).collect();
            if pts.len() >= 2 {
                painter.add(Shape::line(pts, Stroke::new(3.0_f32, color.gamma_multiply(0.85))));
            }
        }
    }

    /// Drops what has gone quiet: strokes and moves whose author stopped sending, editing badges nobody renews.
    pub fn prune(&mut self, now: Instant) {
        self.strokes.retain(|_, s| now.saturating_duration_since(s.last) < STROKE_IDLE);
        self.ghosts.retain(|_, g| now.saturating_duration_since(g.last) < STROKE_IDLE);
        for p in self.people.values_mut() {
            if p.editing.as_ref().is_some_and(|(_, seen)| now.saturating_duration_since(*seen) >= EDITING_IDLE) {
                p.editing = None;
            }
        }
    }
}

impl AgentPresence {
    pub fn new(animate: bool, headless: bool) -> Self {
        Self { animate, headless, state: PresenceState::new(Participant::agent("Claude")), outbox: VecDeque::new(), remote: RemotePresence::default() }
    }

    /// Records an event (always) and animates it (unless disabled or headless).
    pub fn record(&mut self, ev: AgentEvent, anchor: Pt, now: Instant) {
        if self.animate && !self.headless {
            self.state.act(&ev, anchor, now);
        }
        if self.outbox.len() >= OUTBOX_MAX {
            self.outbox.pop_front();
        }
        self.outbox.push_back(ev);
    }

    pub fn paint(&self, painter: &Painter, cam: &Cam, ctx: &egui::Context, page: &Page) {
        let now = Instant::now();
        self.remote.paint(painter, cam, self.remote.now(), page);
        draw_presence(painter, cam, &self.state, now);
        if self.state.is_animating(now) || self.remote.is_animating(now) {
            ctx.request_repaint();
        }
    }
}

fn setting_path() -> std::path::PathBuf {
    crate::platform::data_dir().join("animate_agent")
}

/// Reads the "Animate the agent" setting (on by default).
pub fn load_animate() -> bool {
    std::fs::read_to_string(setting_path()).map(|s| s.trim() != "0").unwrap_or(true)
}

pub fn save_animate(on: bool) {
    let path = setting_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if on { "1" } else { "0" });
}

/* ------------------------------------------------------------------ events from tool calls */

/// Is `tool` one that only reads the board (shown as a look)?
pub fn is_look_tool(tool: &str) -> bool {
    LOOK_TOOLS.contains(&tool)
}

fn push_ids(v: &Value, out: &mut Vec<String>) {
    for key in ["id", "ids", "from", "to"] {
        match v.get(key) {
            Some(Value::String(s)) => out.push(s.clone()),
            Some(Value::Array(a)) => out.extend(a.iter().filter_map(|i| i.as_str().map(str::to_owned))),
            _ => {}
        }
    }
}

/// Objects named by a call (its arguments) or created by it (its result) that still exist on `page`.
fn targets_of(args: &Value, result: &Value, page: &Page) -> Vec<Target> {
    let mut names = Vec::new();
    push_ids(args, &mut names);
    push_ids(result, &mut names);
    let mut out: Vec<Target> = Vec::new();
    for n in names {
        let id = Id(n);
        if out.iter().all(|t| t.id != id)
            && let Some(o) = page.get(&id)
        {
            out.push(Target { id, frame: page.bounds(o) });
        }
    }
    out
}

/// Builds the presence event of a successful tool call. `page` is the board after the call; ids that no longer exist are ignored.
pub fn agent_event(name: &str, tool: &str, args: &Value, result: &Value, page: &Page, at: Instant) -> AgentEvent {
    let mut ev = AgentEvent { name: name.into(), tool: tool.into(), action: Action::Act, steps: Vec::new(), at };
    if LOOK_TOOLS.contains(&tool) {
        ev.action = Action::Look;
    } else if tool == "batch" {
        let ops = args.get("operations").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let results = result.get("results").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        for (op, res) in ops.iter().zip(results) {
            let t = targets_of(op.get("args").unwrap_or(&Value::Null), res, page);
            if !t.is_empty() {
                ev.steps.push(t);
            }
        }
    } else {
        let t = targets_of(args, result, page);
        if !t.is_empty() {
            ev.steps.push(t);
        }
    }
    ev
}

/* ------------------------------------------------------------------ drawing */

/// Arrow outline relative to its tip (screen pixels), and its decomposition into convex triangles.
const ARROW: [(f32, f32); 7] = [(0.0, 0.0), (0.0, 16.5), (4.0, 12.8), (7.2, 20.0), (9.8, 18.9), (6.6, 11.8), (11.5, 11.8)];
/// Screen size of the arrow relative to the outline below.
const ARROW_SCALE: f32 = 1.3;
const ARROW_TRIS: [[usize; 3]; 5] = [[0, 1, 2], [0, 2, 5], [0, 5, 6], [2, 3, 4], [2, 4, 5]];

/// Draws the presence of one participant: halos, selection outline, trail, look pulse, then the arrow with its name badge.
/// Sizes of the cursor and badge are in screen pixels (constant whatever the zoom); frames follow the camera.
pub fn draw_presence(painter: &Painter, cam: &Cam, st: &PresenceState, now: Instant) {
    let now = st.clock(now);
    let a = st.opacity(now);
    let (Some(pos), true) = (st.position_at(now), a > 0.0) else { return };
    let color = st.participant.color;

    for h in &st.halos {
        let Some(age) = now.checked_duration_since(h.start) else { continue };
        let p = age.as_secs_f32() / HALO_LIFE.as_secs_f32();
        if p >= 1.0 {
            continue;
        }
        let r = cam.rect_s(h.frame).expand(3.0 + 7.0 * p);
        let k = (1.0 - p) * a;
        painter.rect_filled(r, 8.0, color.gamma_multiply(0.16 * k));
        painter.rect_stroke(r, 8.0, Stroke::new(1.5 + 2.5 * (1.0 - p), color.gamma_multiply(k)), StrokeKind::Middle);
    }
    for f in &st.selection {
        painter.rect_stroke(cam.rect_s(*f).expand(2.0), 3.0, Stroke::new(2.0_f32, color.gamma_multiply(a)), StrokeKind::Middle);
    }

    // trail: the positions of the last few hundred milliseconds, fading with age
    let mut prev: Option<Pos2> = None;
    for i in (0..=TRAIL_SAMPLES).rev() {
        let age = TRAIL_LIFE.mul_f32(i as f32 / TRAIL_SAMPLES as f32);
        let Some(p) = now.checked_sub(age).and_then(|t| st.position_at(t)).map(|p| cam.s(p)) else { continue };
        if let Some(q) = prev.filter(|q| q.distance(p) > 0.5) {
            let k = 1.0 - age.as_secs_f32() / TRAIL_LIFE.as_secs_f32();
            painter.line_segment([q, p], Stroke::new(1.0 + 3.0 * k, color.gamma_multiply(0.55 * k * a)));
        }
        prev = Some(p);
    }

    let tip = cam.s(pos);
    if let Some(age) = st.look.and_then(|t| now.checked_duration_since(t)).filter(|d| *d < LOOK_LIFE) {
        let p = age.as_secs_f32() / LOOK_LIFE.as_secs_f32();
        painter.circle_stroke(tip, 8.0 + 16.0 * p, Stroke::new(2.0_f32, color.gamma_multiply(0.7 * (1.0 - p) * a)));
    }

    let pts: Vec<Pos2> = ARROW.iter().map(|(x, y)| tip + vec2(*x, *y) * ARROW_SCALE).collect();
    painter.add(Shape::closed_line(pts.clone(), Stroke::new(3.0_f32, Color32::WHITE.gamma_multiply(a))));
    let mut mesh = Mesh::default();
    for p in &pts {
        mesh.colored_vertex(*p, color.gamma_multiply(a));
    }
    for t in ARROW_TRIS {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(Shape::mesh(mesh));

    let galley = painter.layout_no_wrap(st.participant.name.clone(), FontId::proportional(13.0), Color32::WHITE.gamma_multiply(a));
    let badge = egui::Rect::from_min_size(tip + vec2(16.0, 22.0), galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(badge, 9.0, color.gamma_multiply(a));
    painter.rect_stroke(badge, 9.0, Stroke::new(1.5_f32, Color32::WHITE.gamma_multiply(a)), StrokeKind::Outside);
    painter.galley(badge.min + vec2(6.0, 3.0), galley, Color32::WHITE);
}

/// "Anna is typing…" above an object whose text somebody is editing: informative only, nothing is locked.
fn draw_editing_badge(painter: &Painter, frame: egui::Rect, who: &Participant) {
    let color = who.color;
    painter.rect_stroke(frame.expand(3.0), 4.0, Stroke::new(2.0_f32, color), StrokeKind::Middle);
    let galley = painter.layout_no_wrap(tr!("{} is typing…", who.name), FontId::proportional(12.0), Color32::WHITE);
    let badge = egui::Rect::from_min_size(frame.left_top() + vec2(0.0, -galley.size().y - 14.0), galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(badge, 8.0, color);
    painter.galley(badge.min + vec2(6.0, 3.0), galley, Color32::WHITE);
}

/* ------------------------------------------------------------------ following a participant */

/// Delay of the view's glide toward the followed view (the more, the smoother).
const FOLLOW_RATE: f32 = 12.0;

/// Starts or stops following `who` (their page and visible area); the view then moves by itself until the user acts.
pub fn follow(app: &mut App, who: Option<u32>) {
    app.agent.remote.following = who;
    app.agent.remote.follow_set = None;
}

/// Call on every frame before the canvas: the view glides toward the followed participant's, unless the user has just moved it
/// (zoom, scroll, page change, a key) which ends the following.
pub fn follow_frame(app: &mut App, ctx: &egui::Context) {
    let Some(id) = app.agent.remote.following else { return };
    let cur = (app.ed.current.clone(), app.ed.page().view);
    let typing = ctx.wants_keyboard_input();
    let key = !typing && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
    if app.agent.remote.follow_set.as_ref().is_some_and(|s| *s != cur) || app.editing.is_some() || key {
        return follow(app, None);
    }
    let Some(p) = app.agent.remote.people.get(&id) else { return };
    let (page, vp, cursor) = (p.page.clone(), p.viewport, p.state.cursor);
    if app.ed.doc.page(&page).is_some() && app.ed.current != page {
        app.commit_editing();
        app.ed.current = page;
        app.sel.clear();
    }
    let view = app.ed.page().view;
    let (w, h) = (f64::from(app.canvas_size.0.max(100.0)), f64::from(app.canvas_size.1.max(100.0)));
    let target = match (vp, cursor) {
        (Some(vp), _) => {
            let zoom = (w / vp.w.max(1.0)).min(h / vp.h.max(1.0)).clamp(0.05, 8.0);
            let c = vp.center();
            inkboard_core::View { x: w / 2.0 - c.x * zoom, y: h / 2.0 - c.y * zoom, zoom }
        }
        (None, Some(c)) => inkboard_core::View { x: w / 2.0 - c.x * view.zoom, y: h / 2.0 - c.y * view.zoom, ..view },
        (None, None) => return,
    };
    let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
    let k = f64::from(1.0 - (-FOLLOW_RATE * dt).exp());
    let close = (target.x - view.x).abs() < 0.5 && (target.y - view.y).abs() < 0.5 && (target.zoom / view.zoom - 1.0).abs() < 0.002;
    let next = if close {
        target
    } else {
        inkboard_core::View { x: view.x + (target.x - view.x) * k, y: view.y + (target.y - view.y) * k, zoom: view.zoom * (target.zoom / view.zoom).powf(k) }
    };
    app.set_view(next);
    app.agent.remote.follow_set = Some((app.ed.current.clone(), next));
    if !close {
        ctx.request_repaint();
    }
}

/// The bar "Following Anna — Stop following" and, for a guest, the list of the others with their "Follow" button (the host has its own
/// in the Share window).
pub fn follow_ui(app: &mut App, ctx: &egui::Context) {
    let mut set: Option<Option<u32>> = None;
    if app.guest_active() {
        let people: Vec<(u32, String, Color32)> = app.guest.as_ref().map(|g| g.others()).unwrap_or_default();
        if let Some(f) = app.agent.remote.following
            && !people.iter().any(|p| p.0 == f)
        {
            set = Some(None);
        }
        if !people.is_empty() {
            egui::Area::new(egui::Id::new("guest-people")).anchor(egui::Align2::RIGHT_TOP, vec2(-12.0, 52.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    for (id, name, color) in &people {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 5.0, *color);
                            ui.label(name);
                            let on = app.agent.remote.following == Some(*id);
                            if ui.small_button(if on { t("Stop following") } else { t("Follow") }).clicked() {
                                set = Some((!on).then_some(*id));
                            }
                        });
                    }
                });
            });
        }
    }
    if let Some(id) = app.agent.remote.following {
        let name = app.agent.remote.people.get(&id).map_or_else(|| tr!("Participant {}", id), |p| p.state.participant.name.clone());
        egui::Area::new(egui::Id::new("follow-bar")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -16.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr!("Following {}", name));
                    if ui.button(t("Stop following")).clicked() {
                        set = Some(None);
                    }
                });
            });
        });
    }
    if let Some(who) = set {
        follow(app, who);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_core::geom::Rect;

    fn target(id: &str, x: f64, y: f64) -> Target {
        Target { id: Id(id.into()), frame: Rect::new(x, y, 100.0, 50.0) }
    }

    fn event(action: Action, steps: Vec<Vec<Target>>, at: Instant) -> AgentEvent {
        AgentEvent { name: "Claude".into(), tool: "t".into(), action, steps, at }
    }

    #[test]
    fn the_cursor_glides_to_the_center_of_the_target_and_rests_there() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant::agent("Claude"));
        s.act(&event(Action::Act, vec![vec![target("a", 100.0, 100.0)]], t0), Pt::new(0.0, 0.0), t0);
        assert_eq!(s.position_at(t0), Some(Pt::new(0.0, 0.0)));
        let mid = s.position_at(t0 + Duration::from_millis(150)).unwrap();
        assert!(mid.x > 0.0 && mid.x < 150.0, "{mid:?}");
        assert_eq!(s.position_at(t0 + Duration::from_secs(3)), Some(Pt::new(150.0, 125.0)));
    }

    #[test]
    fn a_look_does_not_move_the_cursor() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant::agent("Claude"));
        s.act(&event(Action::Look, vec![], t0), Pt::new(5.0, 6.0), t0);
        assert_eq!(s.position_at(t0 + Duration::from_secs(1)), Some(Pt::new(5.0, 6.0)));
        s.act(&event(Action::Look, vec![], t0), Pt::new(99.0, 99.0), t0);
        assert_eq!(s.cursor, Some(Pt::new(5.0, 6.0)));
        assert!(s.settled_at().is_none());
    }

    #[test]
    fn a_batch_visits_each_group_in_order_with_a_halo_on_every_target() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant::agent("Claude"));
        let steps = vec![vec![target("a", 0.0, 0.0)], vec![target("b", 400.0, 0.0), target("c", 600.0, 0.0)], vec![target("d", 0.0, 400.0)]];
        s.act(&event(Action::Act, steps, t0), Pt::ZERO, t0);
        assert_eq!(s.legs.len(), 3);
        assert_eq!(s.halos.len(), 4);
        assert!(s.legs.windows(2).all(|w| w[0].end <= w[1].start), "legs are sequential");
        assert_eq!(s.legs[1].to, Pt::new(550.0, 25.0), "center of the union of the group");
    }

    #[test]
    fn a_long_batch_is_shown_as_a_bounded_number_of_stops() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant::agent("Claude"));
        let steps: Vec<Vec<Target>> = (0..200).map(|i| vec![target(&format!("o{i}"), f64::from(i) * 10.0, 0.0)]).collect();
        s.act(&event(Action::Act, steps, t0), Pt::ZERO, t0);
        assert!(s.legs.len() <= MAX_STOPS);
        assert_eq!(s.halos.len(), 200);
    }

    #[test]
    fn the_cursor_fades_after_the_idle_delay() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant::agent("Claude"));
        assert_eq!(s.opacity(t0), 0.0);
        s.act(&event(Action::Look, vec![], t0), Pt::ZERO, t0);
        assert_eq!(s.opacity(t0 + Duration::from_secs(1)), 1.0);
        let fading = s.opacity(t0 + IDLE - FADE / 2);
        assert!(fading > 0.0 && fading < 1.0);
        assert_eq!(s.opacity(t0 + IDLE), 0.0);
    }

    #[test]
    fn disabled_or_headless_presence_records_the_event_but_shows_nothing() {
        let t0 = Instant::now();
        for (animate, headless) in [(false, false), (true, true)] {
            let mut p = AgentPresence::new(animate, headless);
            p.record(event(Action::Act, vec![vec![target("a", 0.0, 0.0)]], t0), Pt::ZERO, t0);
            assert_eq!(p.outbox.len(), 1);
            assert!(p.state.cursor.is_none() && p.state.last_activity.is_none());
        }
    }

    #[test]
    fn remote_strokes_end_with_their_gesture_or_after_the_safety_delay_and_a_sign_of_life_renews_them() {
        let t0 = Instant::now();
        let mut r = RemotePresence::default();
        r.add_stroke(Id("s1".into()), 1, vec![Pt::ZERO, Pt::new(5.0, 5.0)], t0);
        r.add_stroke(Id("s2".into()), 2, vec![Pt::ZERO], t0);
        r.settle(1);
        assert_eq!(r.strokes.len(), 1, "the gesture of author 1 is over");
        r.touch(2, t0 + Duration::from_secs(2));
        r.prune(t0 + Duration::from_secs(4));
        assert_eq!(r.strokes.len(), 1, "author 2 was heard from 2 s ago");
        r.prune(t0 + Duration::from_secs(6));
        assert!(r.strokes.is_empty(), "nobody renewed it");
    }

    #[test]
    fn a_resting_remote_cursor_fades_even_though_its_position_is_repeated() {
        let t0 = Instant::now();
        let mut s = PresenceState::new(Participant { id: "g".into(), name: "Anna".into(), color: Color32::RED, kind: ParticipantKind::Human });
        s.glide_to(Pt::new(10.0, 10.0), t0);
        s.glide_to(Pt::new(10.0, 10.0), t0 + Duration::from_secs(4)); // a keep-alive
        assert_eq!(s.opacity(t0 + IDLE), 0.0);
    }

    #[test]
    fn the_outbox_is_bounded() {
        let t0 = Instant::now();
        let mut p = AgentPresence::new(false, false);
        for _ in 0..OUTBOX_MAX + 10 {
            p.record(event(Action::Look, vec![], t0), Pt::ZERO, t0);
        }
        assert_eq!(p.outbox.len(), OUTBOX_MAX);
    }
}
