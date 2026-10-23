//! Cost of a frame of the whole application with remote participants (see `tests_crowd::crowd_ui_frames`).
//!
//! The host is the real `App` in the egui test harness (a full `update()` per frame, no window, no GPU), sharing its board through the
//! in-memory network; the participants are `inkboard_crowd` guests (real clients, busy profile: 30 Hz cursors, strokes and moves in
//! progress in front of the host, edits, texts). Only `h.step()` is timed.

use crate::session_host::{HostAddress, HostTransport, SessionNet};
use crate::tests::T;
use inkboard_core::Editor;
use inkboard_core::model::{Document, View};
use inkboard_crowd::board::{BoardSpec, seed_document};
use inkboard_crowd::guest::{GuestEnv, GuestPlan, SimGuest};
use inkboard_crowd::link::{Dialer, LinkSpec, ManualClock};
use inkboard_crowd::profile::Profile;
use inkboard_crowd::sim::SimDialer;
use inkboard_crowd::stats::Summary;
use inkboard_session::crypto::Link;
use inkboard_session::proto::Role;
use inkboard_session::sim::{NetConfig, SimNet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const HOST_ID: &str = "host";

struct SimShare(SimNet);

impl SessionNet for SimShare {
    fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String> {
        Ok((Box::new(self.0.transport(HOST_ID)), HostAddress { host: HOST_ID.to_owned(), relays: Vec::new() }))
    }
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// A view that shows the whole page (the worst case for drawing), as "fit all" would.
fn fit_all(doc: &Document) -> View {
    let (mut x1, mut y1, mut x0, mut y0) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for o in &doc.pages[0].objects {
        if let Some(f) = o.frame() {
            x0 = x0.max(f.x + f.w);
            y0 = y0.max(f.y + f.h);
            x1 = x1.min(f.x);
            y1 = y1.min(f.y);
        }
    }
    let zoom = (1000.0 / (x0 - x1).max(1.0)).min(650.0 / (y0 - y1).max(1.0));
    View { x: 300.0 - x1 * zoom, y: 150.0 - y1 * zoom, zoom }
}

/// Frames of the application (ms) with `participants` guests, on `doc`, at `view`.
fn measure(doc: &Document, participants: usize, frames: usize, view: Option<View>, seed: u64) -> Summary {
    let mut t = T::new();
    t.app().ed = Editor::new(doc.clone());
    let view = view.unwrap_or(View { x: 300.0 - 600.0, y: 150.0 - 400.0, zoom: 1.0 });
    t.app().set_view(view);
    t.h.step();
    let net = SimNet::new(seed, NetConfig::ideal());
    let clock = Arc::new(AtomicU64::new(0));
    let mut guests: Vec<SimGuest> = Vec::new();
    let manual = Arc::new(ManualClock::default());
    if participants > 0 {
        t.app().share.net = Some(Box::new(SimShare(net.clone())));
        t.app().share.test_clock = Some(Arc::clone(&clock));
        t.app().share.max_participants = participants + 2;
        crate::share_ui::start(t.app());
        assert!(t.app().share.is_running(), "sharing did not start: {:?}", t.app().share.error);
        let link = {
            let a = t.app();
            a.share.host.as_ref().expect("a session").link(Role::Edit, a.share.addr.as_ref().expect("an address")).expect("a link")
        };
        t.app().share.host.as_mut().expect("a session").config_mut().max_pending = participants + 8;
        let ticket = Link::parse(&link).expect("a link").ticket;
        let profile =
            Profile { ramp_s: 0.0, slow_fraction: 0.0, viewer_fraction: 0.0, fetch_images: false, cursor_hz: 30.0, cursor_active: 1.0, ..Profile::busy() };
        let env = Arc::new(GuestEnv { clock: manual.clone(), profile, machine: 1, edit_ticket: ticket.clone(), view_ticket: None, deterministic: true, seed });
        for i in 0..participants {
            let plan = GuestPlan { index: i, name: format!("G{i}"), edit: true, link: LinkSpec::default(), stuck_at_ms: None };
            let dialer: Box<dyn Dialer> = Box::new(SimDialer::new(net.transport(&format!("guest-{i}")), HOST_ID, ticket.clone()));
            let mut g = SimGuest::new(Arc::clone(&env), plan, dialer);
            g.pin_page(0);
            g.pin_area(inkboard_core::geom::Pt::new(900.0, 600.0));
            g.start(0);
            guests.push(g);
        }
    }
    let mut now = 0u64;
    let mut times = Vec::with_capacity(frames);
    let warmup = if participants > 0 { 400 } else { 20 };
    for f in 0..warmup + frames {
        now += 16;
        net.advance(16);
        clock.store(now, Ordering::SeqCst);
        manual.set(now);
        for g in &mut guests {
            g.step(now);
        }
        if let Some(h) = t.app().share.host.as_mut() {
            for p in h.pending() {
                h.accept(p.request, Role::Edit);
            }
        }
        let started = Instant::now();
        t.h.step();
        if f >= warmup {
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    Summary::of(&times)
}

pub fn run() {
    let objects: usize = env_or("CROWD_OBJECTS", 5_000);
    let frames: usize = env_or("CROWD_FRAMES", 300);
    let counts: Vec<usize> =
        std::env::var("CROWD_PARTICIPANTS").unwrap_or_else(|_| "0,5,10,20,50".to_owned()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let doc = seed_document(&BoardSpec::sized(objects, 1));
    let fit = fit_all(&doc);
    println!("\n| board | view | participants | frame ms p50 | p95 | p99 | max |\n|---|---|---|---|---|---|---|");
    for (name, view) in [("cluster at 100 %", None), ("whole page", Some(fit))] {
        for &n in &counts {
            let s = measure(&doc, n, frames, view, 7);
            println!("| {objects} objects | {name} | {n} | {:.2} | {:.2} | {:.2} | {:.2} |", s.p50, s.p95, s.p99, s.max);
        }
    }
}
