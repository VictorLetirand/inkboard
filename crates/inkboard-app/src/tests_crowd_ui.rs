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
    let (rev, build, n) = {
        let a = t.app();
        (a.ed.revision(), a.caches.build_ms, a.ed.page().objects.len())
    };
    eprintln!("[ui] revision {rev} / index rebuild {build:.1} ms / {n} objects on the page");
    Summary::of(&times)
}

/// Frames of a GUEST application (the one that is measured) on `doc`, while `participants` other guests work (real iroh transport on the
/// loopback interface, the host is the real application in the same process): the mirror of the board is refreshed as modifications arrive.
fn measure_guest(doc: &Document, participants: usize, frames: usize) -> Summary {
    use crate::i18n::{Lang, set_for_thread};
    use crate::session_guest::GuestConfig;
    use crate::session_net::{GuestNet, IrohHostNet, NetDiagnoser};
    use inkboard_crowd::fleet::{FleetConfig, run_fleet};
    use inkboard_crowd::link::RealClock;
    use inkboard_crowd::net::{IrohDialer, Mode};
    use inkboard_net::NetConfig as IrohCfg;
    set_for_thread(Some(Lang::En));
    let mut host = T::new();
    host.app().ed = Editor::new(doc.clone());
    host.app().share.net = Some(Box::new(IrohHostNet::new(IrohCfg::loopback())));
    host.app().share.max_participants = participants + 5;
    crate::share_ui::start(host.app());
    for _ in 0..2000 {
        host.h.step();
        if host.app().share.is_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(host.app().share.is_running(), "sharing did not start: {:?}", host.app().share.error);
    let link = {
        let a = host.app();
        a.share.host.as_ref().expect("a session").link(Role::Edit, a.share.addr.as_ref().expect("an address")).expect("a link")
    };
    host.app().share.host.as_mut().expect("a session").config_mut().max_pending = participants + 8;

    let dir = std::env::temp_dir().join(format!("inkboard-crowd-ui-{}", std::process::id()));
    let mut guest = T::new();
    let net = GuestNet {
        net: IrohCfg { connect_timeout: std::time::Duration::from_secs(10), ..IrohCfg::loopback() },
        session: GuestConfig::default(),
        diagnoser: Arc::new(NetDiagnoser { timeout: std::time::Duration::from_secs(3), trusted: Vec::new(), allow_private: true, relays_dir: None }),
        relays_dir: None,
        watch_network: false,
    };
    guest.app().join.factory = Some(net.into_factory());
    guest.app().join.name_dir = Some(dir.clone());
    guest.app().open_join_window(Some(link.clone()));
    if let Some(w) = guest.app().join.window.as_mut() {
        w.name = "Measured".to_owned();
    }
    guest.app().submit_join();

    // the other guests, in threads of this process
    let fleet = (participants > 0).then(|| {
        let ticket = Link::parse(&link).expect("a link").ticket;
        let profile = Profile { ramp_s: 2.0, slow_fraction: 0.0, viewer_fraction: 0.0, fetch_images: false, ..Profile::busy() };
        let env = Arc::new(GuestEnv {
            clock: Arc::new(RealClock::default()),
            profile,
            machine: 3,
            edit_ticket: ticket.clone(),
            view_ticket: None,
            deterministic: false,
            seed: 11,
        });
        let cfg = FleetConfig {
            guests: participants,
            first_index: 1000,
            threads: 2,
            machine_name: "ui".to_owned(),
            t0_epoch_ms: None,
            duration_s: 60.0,
            settle_s: 0.0,
            out_dir: None,
            raw: false,
            transport: "loopback".to_owned(),
        };
        std::thread::spawn(move || {
            let make: Arc<dyn Fn(usize) -> Box<dyn Dialer> + Send + Sync> =
                Arc::new(move |_| Box::new(IrohDialer::new(Mode::Loopback, ticket.clone()).expect("an endpoint")));
            run_fleet(&cfg, env, make)
        })
    });

    let mut times = Vec::with_capacity(frames);
    let mut turns = 0usize;
    let mut joined_at = None;
    while times.len() < frames && turns < 20_000 {
        turns += 1;
        host.h.step();
        if let Some(h) = host.app().share.host.as_mut() {
            for p in h.pending() {
                h.accept(p.request, Role::Edit);
            }
        }
        let started = Instant::now();
        guest.h.step();
        let took = started.elapsed().as_secs_f64() * 1000.0;
        let ready = guest.app().guest.as_ref().is_some_and(|g| g.mirror_ready());
        if ready {
            let at = *joined_at.get_or_insert(turns);
            if turns > at + 300 {
                times.push(took); // after the first 300 turns: the board is in, the others are at work
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    drop(guest);
    drop(host);
    if let Some(f) = fleet {
        let _ = f.join();
    }
    let _ = std::fs::remove_dir_all(dir);
    Summary::of(&times)
}

pub fn run_guest() {
    let objects: usize = env_or("CROWD_OBJECTS", 5_000);
    let frames: usize = env_or("CROWD_FRAMES", 300);
    let counts: Vec<usize> =
        std::env::var("CROWD_PARTICIPANTS").unwrap_or_else(|_| "0,10,20".to_owned()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let doc = seed_document(&BoardSpec::sized(objects, 1));
    println!("\n| board | other participants at work | guest frame ms p50 | p95 | p99 | max |\n|---|---|---|---|---|---|");
    for &n in &counts {
        let s = measure_guest(&doc, n, frames);
        println!("| {objects} objects | {n} | {:.2} | {:.2} | {:.2} | {:.2} |", s.p50, s.p95, s.p99, s.max);
    }
}

/// `CROWD_ONLY=box|pen|connector|table|image`: keeps one kind of object (to find what a frame spends its time on).
fn only(mut doc: Document) -> Document {
    use inkboard_core::model::Kind;
    let Ok(kind) = std::env::var("CROWD_ONLY") else { return doc };
    for p in &mut doc.pages {
        p.objects.retain(|o| {
            matches!(
                (kind.as_str(), &o.kind),
                ("box" | "boxconn", Kind::Box(_))
                    | ("boxconn", Kind::Connector(_))
                    | ("boxpen", Kind::Box(_) | Kind::Pen(_))
                    | ("boximg", Kind::Box(_) | Kind::Image(_))
                    | ("pen", Kind::Pen(_))
                    | ("connector", Kind::Connector(_))
                    | ("table", Kind::Table(_))
                    | ("image", Kind::Image(_))
            )
        });
    }
    doc
}

pub fn run() {
    let objects: usize = env_or("CROWD_OBJECTS", 5_000);
    let frames: usize = env_or("CROWD_FRAMES", 300);
    let counts: Vec<usize> =
        std::env::var("CROWD_PARTICIPANTS").unwrap_or_else(|_| "0,5,10,20,50".to_owned()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let doc = only(seed_document(&BoardSpec::sized(objects, 1)));
    let fit = fit_all(&doc);
    println!("\n| board | view | participants | frame ms p50 | p95 | p99 | max |\n|---|---|---|---|---|---|---|");
    let mut views = vec![("cluster at 100 %", None)];
    if env_or("CROWD_WHOLE_PAGE", 0) != 0 {
        views.push(("whole page", Some(fit))); // decodes every photo of the board: slow, off by default
    }
    for (name, view) in views {
        for &n in &counts {
            let s = measure(&doc, n, frames, view, 7);
            println!("| {objects} objects | {name} | {n} | {:.2} | {:.2} | {:.2} | {:.2} |", s.p50, s.p95, s.p99, s.max);
        }
    }
}
