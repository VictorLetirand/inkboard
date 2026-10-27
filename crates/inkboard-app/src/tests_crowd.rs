//! Load test of the host (`context/mcp-remote/test-charge.md`). Everything here is `#[ignore]`: it is a measuring tool, started on
//! purpose with environment variables (see `host_from_env`), not a test that runs with the others. No assertion is made on a duration.
//!
//! `crowd_host` starts a REAL host (the session host and the iroh transport) on a seed board and serves guests started in ANOTHER process
//! (`crowd guests`, crate `inkboard-crowd`), accepting every request. It measures what the host feels: time of each turn of its loop,
//! CPU and memory of the process, bytes in and out, size of the send queues, size of the log of modifications. `crowd_ui_frames`
//! measures the cost of one frame of the whole application with N remote participants and gestures, without a window.

use crate::session_host::{HostAddress, HostConfig, HostEvent, HostStats, HostTransport, SessionHost, SessionNet};
use crate::tests::T;
use inkboard_core::geom::{Frame, Pt, Rect};
use inkboard_core::model::{BoxGeom, Id, Label};
use inkboard_core::style::{Align, Style};
use inkboard_core::{Editor, Object};
use inkboard_crowd::board::{BoardSpec, asset_bytes, object_count, seed_document};
use inkboard_crowd::digest::doc_digest;
use inkboard_crowd::net::Mode;
use inkboard_crowd::proc::ProcSampler;
use inkboard_crowd::report::HostReport;
use inkboard_crowd::rng::Rng;
use inkboard_crowd::stats::Samples;
use inkboard_net::{IrohConn, IrohTransport, QueueProbe};
use inkboard_session::error::Result as NetResult;
use inkboard_session::proto::{Presence, Role};
use inkboard_session::transport::{Connection, Transport};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/* ------------------------------------------------------------------------------------------------------ counting transport */

#[derive(Default)]
struct NetStats {
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
    tx_frames: AtomicU64,
    rx_frames: AtomicU64,
    errors: Mutex<BTreeMap<String, u64>>,
    probes: Mutex<Vec<(QueueProbe, Arc<AtomicBool>)>>,
}

impl NetStats {
    /// Bytes waiting in the send queue of the worst connection, and of all together.
    fn queues(&self) -> (usize, usize) {
        let mut probes = self.probes.lock().unwrap_or_else(PoisonError::into_inner);
        probes.retain(|(_, alive)| alive.load(Ordering::Relaxed));
        let sizes: Vec<usize> = probes.iter().map(|(p, _)| p.out_bytes()).collect();
        (sizes.iter().copied().max().unwrap_or(0), sizes.iter().sum())
    }
}

struct CountedConn {
    inner: IrohConn,
    s: Arc<NetStats>,
    alive: Arc<AtomicBool>,
}

impl Drop for CountedConn {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

impl Connection for CountedConn {
    fn peer(&self) -> &str {
        self.inner.peer()
    }
    fn send(&mut self, frame: &[u8]) -> NetResult<()> {
        match self.inner.send(frame) {
            Ok(()) => {
                self.s.tx_bytes.fetch_add(frame.len() as u64, Ordering::Relaxed);
                self.s.tx_frames.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                *self.s.errors.lock().unwrap_or_else(PoisonError::into_inner).entry(e.to_string().chars().take(70).collect()).or_insert(0) += 1;
                Err(e)
            }
        }
    }
    fn recv(&mut self) -> Option<Vec<u8>> {
        let f = self.inner.recv()?;
        self.s.rx_bytes.fetch_add(f.len() as u64, Ordering::Relaxed);
        self.s.rx_frames.fetch_add(1, Ordering::Relaxed);
        Some(f)
    }
    fn is_open(&self) -> bool {
        self.inner.is_open()
    }
    fn close(&mut self) {
        self.inner.close();
    }
}

struct CountedTransport {
    inner: IrohTransport,
    s: Arc<NetStats>,
}

impl HostTransport for CountedTransport {
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        let conn = Transport::accept(&mut self.inner)?;
        let alive = Arc::new(AtomicBool::new(true));
        self.s.probes.lock().unwrap_or_else(PoisonError::into_inner).push((conn.probe(), Arc::clone(&alive)));
        Some(Box::new(CountedConn { inner: conn, s: Arc::clone(&self.s), alive }))
    }
}

/// The network side of "Share" for the `app` mode: the counting transport.
struct CountedNet {
    mode: Mode,
    s: Arc<NetStats>,
}

fn open_host(mode: Mode, s: &Arc<NetStats>) -> Result<(Box<dyn HostTransport>, HostAddress), String> {
    let transport = IrohTransport::host(mode.host_config()).map_err(|e| e.to_string())?;
    let host = transport.local_id().ok_or("no identity")?;
    let relays = transport.hints().map_err(|e| e.to_string())?;
    Ok((Box::new(CountedTransport { inner: transport, s: Arc::clone(s) }), HostAddress { host, relays }))
}

impl SessionNet for CountedNet {
    fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String> {
        open_host(self.mode, &self.s)
    }
}

/* ------------------------------------------------------------------------------------------------------------ the bench */

struct Bench {
    out: PathBuf,
    objects: usize,
    seed: u64,
    mode: Mode,
    max_participants: usize,
    max_pending: usize,
    duration_s: f64,
    settle_s: f64,
    grace_s: f64,
    warmup_s: f64,
    t0_epoch_ms: Option<u64>,
    host_activity: bool,
    app: bool,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn host_from_env() -> Option<Bench> {
    let out = PathBuf::from(std::env::var("CROWD_OUT").ok()?);
    Some(Bench {
        out,
        objects: env_or("CROWD_OBJECTS", 1_000),
        seed: env_or("CROWD_SEED", 1),
        mode: Mode::parse(&std::env::var("CROWD_NET").unwrap_or_else(|_| "loopback".to_owned())).unwrap_or(Mode::Loopback),
        max_participants: env_or("CROWD_MAX", 120),
        max_pending: env_or("CROWD_PENDING", 64),
        duration_s: env_or("CROWD_DURATION", 60.0),
        settle_s: env_or("CROWD_SETTLE", 10.0),
        grace_s: env_or("CROWD_GRACE", 120.0),
        warmup_s: env_or("CROWD_WARMUP", 15.0),
        t0_epoch_ms: std::env::var("CROWD_T0").ok().and_then(|v| v.trim().parse().ok()),
        host_activity: env_or("CROWD_HOST_ACTIVITY", 1) != 0,
        app: env_or("CROWD_APP", 0) != 0,
    })
}

/// The host's side of the scene: either the bare session host on an editor, or the whole application.
enum Driver {
    Bare { ed: Box<Editor>, host: Box<SessionHost> },
    App(Box<T>),
}

impl Driver {
    fn host(&mut self) -> &mut SessionHost {
        match self {
            Driver::Bare { host, .. } => host,
            Driver::App(t) => t.app().share.host.as_mut().expect("a session"),
        }
    }

    fn editor(&mut self) -> &mut Editor {
        match self {
            Driver::Bare { ed, .. } => ed,
            Driver::App(t) => &mut t.app().ed,
        }
    }

    /// One turn of the host: what the application does on every frame.
    fn turn(&mut self, now_ms: u64) {
        match self {
            Driver::Bare { ed, host } => host.poll(ed, now_ms),
            Driver::App(t) => t.h.step(),
        }
    }
}

/// What the host person does now and then: moves an object, adds a note, deletes the last note.
fn host_edit(ed: &mut Editor, rng: &mut Rng, notes: &mut Vec<Id>) {
    let roll = rng.f64();
    let page = ed.page();
    if roll < 0.5 && !page.objects.is_empty() {
        let id = page.objects[rng.below(page.objects.len())].id.clone();
        ed.translate(&[id], Pt::new(rng.range(-60.0, 60.0), rng.range(-60.0, 60.0)));
    } else if roll < 0.9 || notes.is_empty() {
        let o = Object::new_box(
            BoxGeom::Sticky,
            Frame::new(rng.range(0.0, 3000.0), rng.range(0.0, 2000.0), 150.0, 100.0),
            Style::default(),
            Label::new("host note", 18.0, Align::Center),
        );
        notes.push(ed.insert(o));
    } else if let Some(id) = notes.pop() {
        ed.remove(&[id]);
    }
}

fn reason_key(s: &str) -> String {
    // "sending too fast (frames)" -> keep the text without numbers
    s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>().trim().chars().take(60).collect()
}

fn run_host(b: &Bench) {
    std::fs::create_dir_all(&b.out).expect("the output folder");
    let spec = BoardSpec::sized(b.objects, b.seed);
    let board_started = Instant::now();
    let mut doc = seed_document(&spec);
    if std::env::var_os("CROWD_NEW_FIELDS").is_some() {
        inkboard_crowd::board::add_v4_fields(&mut doc, b.seed);
    }
    eprintln!(
        "[host] board of {} objects, {} pages, {:.1} MiB of images, built in {:.1} s",
        object_count(&doc),
        doc.pages.len(),
        asset_bytes(&doc) as f64 / 1_048_576.0,
        board_started.elapsed().as_secs_f64()
    );
    let board_json = serde_json::to_vec(&doc.pages).map_or(0, |v| v.len());
    let (board_objects, board_pages, board_assets, board_images) = (object_count(&doc), doc.pages.len(), asset_bytes(&doc), doc.assets.len());
    let stats = Arc::new(NetStats::default());
    let cfg = HostConfig { max_participants: b.max_participants, max_pending: b.max_pending, ..HostConfig::default() };

    let (mut driver, addr) = if b.app {
        let mut t = T::new();
        t.app().ed = Editor::new(doc);
        t.app().share.net = Some(Box::new(CountedNet { mode: b.mode, s: Arc::clone(&stats) }));
        t.app().share.max_participants = b.max_participants;
        crate::share_ui::start(t.app());
        for _ in 0..2000 {
            t.h.step();
            if t.app().share.is_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(t.app().share.is_running(), "sharing did not start: {:?}", t.app().share.error);
        let addr = t.app().share.addr.clone().expect("an address");
        t.app().share.host.as_mut().expect("a session").config_mut().max_pending = b.max_pending;
        (Driver::App(Box::new(t)), addr)
    } else {
        let ed = Editor::new(doc);
        let (transport, addr) = open_host(b.mode, &stats).expect("the host endpoint");
        let host = SessionHost::start(cfg, &ed, transport).expect("a session");
        (Driver::Bare { ed: Box::new(ed), host: Box::new(host) }, addr)
    };
    let edit_link = driver.host().link(Role::Edit, &addr).expect("a link");
    let view_link = driver.host().link(Role::View, &addr).expect("a link");
    let t0 = b.t0_epoch_ms.unwrap_or_else(|| epoch_ms() + (b.warmup_s * 1000.0) as u64);
    std::fs::write(b.out.join("t0"), t0.to_string()).expect("t0");
    std::fs::write(b.out.join("view.link"), &view_link).expect("view link");
    std::fs::write(b.out.join("edit.link"), &edit_link).expect("edit link"); // written last: the signal that the host is ready
    eprintln!("[host] ready ({} mode, {}): t0={t0}, links in {}", b.mode.name(), if b.app { "app" } else { "bare" }, b.out.display());

    let activity_end = t0 + (b.duration_s * 1000.0) as u64;
    let auto_stop = activity_end + ((b.settle_s + b.grace_s) * 1000.0) as u64;
    let mut sampler = ProcSampler::new();
    let rss_start = sampler.sample().rss_mib;
    let mut rng = Rng::new(b.seed ^ 0xBEEF);
    let mut notes: Vec<Id> = Vec::new();
    let started = Instant::now();
    let period = Duration::from_millis(16);
    let (mut turns, mut gaps) = (Samples::default(), Samples::default());
    let (mut sec_turns, mut cpu, mut rss, mut machine, mut tx_s, mut rx_s, mut queue_worst) =
        (Samples::default(), Samples::default(), Samples::default(), Samples::default(), Samples::default(), Samples::default(), Samples::default());
    let mut timeline = String::from(
        "t_s,turn_p50_ms,turn_p99_ms,turn_max_ms,cpu_cores,rss_mib,machine_cpu_pct,tx_mib_s,rx_mib_s,members,connections,queue_worst_kib,queue_total_kib,seq,log_entries,log_kib,tracked_objects,warned,excluded\n",
    );
    let mut r = HostReport::default();
    let (mut last_sample, mut last_turn_start) = (Instant::now(), Instant::now());
    let (mut last_tx, mut last_rx) = (0u64, 0u64);
    let (mut next_host_edit, mut next_presence) = (0u64, 0u64);
    let mut host_edits = 0u64;
    let mut last_stats = HostStats::default();
    let mut peak_queue_total = 0usize;
    loop {
        let turn_start = Instant::now();
        gaps.add(turn_start.duration_since(last_turn_start).as_secs_f64() * 1000.0);
        last_turn_start = turn_start;
        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let wall = epoch_ms();
        driver.turn(now_ms);
        // answer the requests: everybody is let in (a person would click "Accept")
        let pending = driver.host().pending();
        for p in pending {
            driver.host().accept(p.request, Role::Edit);
        }
        let events = driver.host().take_events();
        for ev in events {
            match ev {
                HostEvent::Joined { .. } => r.joined += 1,
                HostEvent::Left { .. } => r.left += 1,
                HostEvent::Excluded { reason, .. } => {
                    r.excluded += 1;
                    *r.excluded_reasons.entry(reason_key(&reason)).or_insert(0) += 1;
                }
                HostEvent::Warned { reason, .. } => {
                    r.warned += 1;
                    *r.warnings.entry(reason_key(&reason)).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        // the host person
        if b.host_activity && wall >= t0 && wall < activity_end {
            if now_ms >= next_host_edit {
                next_host_edit = now_ms + 1_500 + rng.below(4_000) as u64;
                host_edit(driver.editor(), &mut rng, &mut notes);
                host_edits += 1;
            }
            if now_ms >= next_presence && matches!(driver, Driver::Bare { .. }) {
                next_presence = now_ms + 50;
                let page = driver.editor().page().id.clone();
                let c = Pt::new(rng.range(0.0, 3000.0), rng.range(0.0, 2000.0));
                driver.host().send_host_presence(Presence {
                    participant: None,
                    page,
                    cursor: Some(c),
                    selection: Vec::new(),
                    tool: Some("select".to_owned()),
                    editing: None,
                    viewport: Some(Rect::new(c.x - 700.0, c.y - 450.0, 1400.0, 900.0)),
                });
            }
        }
        let busy = turn_start.elapsed().as_secs_f64() * 1000.0;
        turns.add(busy);
        sec_turns.add(busy);
        // once a second: sample the process and the queues
        if last_sample.elapsed() >= Duration::from_secs(1) {
            let dt = last_sample.elapsed().as_secs_f64();
            last_sample = Instant::now();
            let p = sampler.sample();
            let (tx, rx) = (stats.tx_bytes.load(Ordering::Relaxed), stats.rx_bytes.load(Ordering::Relaxed));
            let (txs, rxs) = ((tx - last_tx) as f64 / 1_048_576.0 / dt, (rx - last_rx) as f64 / 1_048_576.0 / dt);
            (last_tx, last_rx) = (tx, rx);
            let (worst, total) = stats.queues();
            peak_queue_total = peak_queue_total.max(total);
            let hs = driver.host().stats();
            last_stats = hs;
            r.members_peak = r.members_peak.max(hs.members);
            r.connections_peak = r.connections_peak.max(hs.connections);
            let ts = sec_turns.summary();
            let _ = writeln!(
                timeline,
                "{:.0},{:.2},{:.2},{:.2},{:.2},{:.1},{:.1},{txs:.3},{rxs:.3},{},{},{:.0},{:.0},{},{},{:.0},{},{},{}",
                started.elapsed().as_secs_f64(),
                ts.p50,
                ts.p99,
                ts.max,
                p.cpu_cores,
                p.rss_mib,
                p.machine_cpu_pct,
                hs.members,
                hs.connections,
                worst as f64 / 1024.0,
                total as f64 / 1024.0,
                hs.seq,
                hs.log_entries,
                hs.log_bytes as f64 / 1024.0,
                hs.tracked_objects,
                r.warned,
                r.excluded
            );
            sec_turns = Samples::default();
            cpu.add(p.cpu_cores);
            rss.add(p.rss_mib);
            machine.add(p.machine_cpu_pct);
            tx_s.add(txs);
            rx_s.add(rxs);
            queue_worst.add(worst as f64 / 1024.0);
            r.peak_cpu_cores = r.peak_cpu_cores.max(p.cpu_cores);
            r.peak_rss_mib = r.peak_rss_mib.max(p.rss_mib);
            if started.elapsed().as_secs().is_multiple_of(10) {
                eprintln!(
                    "[host] t={:.0}s members={} conns={} cpu={:.2} rss={:.0} MiB tx={txs:.1} MiB/s rx={rxs:.1} MiB/s turn p99={:.1} ms queue worst={:.0} KiB seq={}",
                    started.elapsed().as_secs_f64(),
                    hs.members,
                    hs.connections,
                    p.cpu_cores,
                    p.rss_mib,
                    ts.p99,
                    worst as f64 / 1024.0,
                    hs.seq
                );
            }
        }
        if b.out.join("stop").exists() || wall >= auto_stop {
            break;
        }
        if let Some(rest) = period.checked_sub(turn_start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    // the guests are gone (or the time is up): last numbers
    for _ in 0..10 {
        driver.turn(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
        std::thread::sleep(Duration::from_millis(16));
    }
    let hs = driver.host().stats();
    let over = |s: &[f64], lim: f64| 100.0 * s.iter().filter(|x| **x > lim).count() as f64 / s.len().max(1) as f64;
    r.board_objects = board_objects;
    r.board_pages = board_pages;
    r.board_json_mib = board_json as f64 / 1_048_576.0;
    r.board_asset_mib = board_assets as f64 / 1_048_576.0;
    r.board_images = board_images;
    r.transport = b.mode.name().to_owned();
    r.mode = if b.app { "app" } else { "bare" }.to_owned();
    r.max_participants = b.max_participants;
    r.host_activity = b.host_activity;
    r.started_epoch_ms = epoch_ms();
    r.t0_epoch_ms = t0;
    r.duration_s = b.duration_s;
    r.turn_ms = turns.summary();
    r.turn_gap_ms = gaps.summary();
    r.turn_over_16_pct = over(&turns.0, 16.0);
    r.turn_over_50_pct = over(&turns.0, 50.0);
    r.cpu_cores = cpu.summary();
    r.rss_mib = rss.summary();
    r.rss_start_mib = rss_start;
    r.rss_end_mib = sampler.sample().rss_mib;
    r.machine_cpu_pct = machine.summary();
    r.tx_mib = stats.tx_bytes.load(Ordering::Relaxed) as f64 / 1_048_576.0;
    r.rx_mib = stats.rx_bytes.load(Ordering::Relaxed) as f64 / 1_048_576.0;
    r.tx_mib_s = tx_s.summary();
    r.rx_mib_s = rx_s.summary();
    r.frames_tx = stats.tx_frames.load(Ordering::Relaxed);
    r.frames_rx = stats.rx_frames.load(Ordering::Relaxed);
    r.send_errors = stats.errors.lock().unwrap_or_else(PoisonError::into_inner).clone();
    r.queue_kib_worst = queue_worst.summary();
    r.queue_kib_total_max = peak_queue_total as f64 / 1024.0;
    r.seq_final = hs.seq;
    r.log_entries = hs.log_entries;
    r.log_kib = hs.log_bytes as f64 / 1024.0;
    r.tracked_objects = hs.tracked_objects;
    let _ = last_stats;
    let doc = driver.editor().doc.clone();
    r.objects_final = object_count(&doc);
    r.digest = doc_digest(&doc);
    r.host_edits = host_edits;
    std::fs::write(b.out.join("host.json"), serde_json::to_vec_pretty(&r).expect("json")).expect("host.json");
    std::fs::write(b.out.join("host-timeline.csv"), timeline).expect("timeline");
    eprintln!(
        "[host] done: {} turns, p99 {:.1} ms, peak cpu {:.2}, peak rss {:.0} MiB, digest {}",
        turns.len(),
        r.turn_ms.p99,
        r.peak_cpu_cores,
        r.peak_rss_mib,
        &r.digest[..12]
    );
    match driver {
        Driver::Bare { mut host, .. } => host.stop(),
        Driver::App(mut t) => crate::share_ui::stop(t.app()),
    }
}

/// The host of a load test. Environment: `CROWD_OUT` (folder; required), `CROWD_OBJECTS` (1000, 5000, 20000), `CROWD_SEED`,
/// `CROWD_NET` (loopback, lan, relay), `CROWD_MAX` (participants, 120), `CROWD_PENDING`, `CROWD_DURATION`, `CROWD_SETTLE`, `CROWD_WARMUP`
/// (seconds before the guests start), `CROWD_T0` (epoch ms), `CROWD_HOST_ACTIVITY` (1/0), `CROWD_APP` (1: the whole application).
/// It stops when `<CROWD_OUT>/stop` appears.
#[test]
#[ignore = "load test tool: see context/mcp-remote/test-charge.md"]
fn crowd_host() {
    let Some(b) = host_from_env() else {
        eprintln!("CROWD_OUT is not set: nothing to do");
        return;
    };
    run_host(&b);
}

/* ---------------------------------------------------------------------------------------------------------- frames of the UI */

/// Cost of one frame of the whole application on a seed board, without a window: alone, then hosting a session with N participants
/// whose cursors move and whose strokes are drawn in front of the host (simulated guests on the in-memory network, deterministic
/// traffic). Prints a markdown table. `CROWD_OBJECTS` (default 5000), `CROWD_PARTICIPANTS` (default `0,5,10,20,50`), `CROWD_FRAMES` (300).
#[test]
#[ignore = "measuring tool: see context/mcp-remote/test-charge.md"]
fn crowd_ui_frames() {
    crate::tests_crowd_ui::run();
}

/// Cost of one frame of a GUEST application on a seed board while other guests work (real transport on the loopback interface, the host
/// is the real application in the same process): the mirror of the board is refreshed as modifications arrive. `CROWD_OBJECTS` (5000),
/// `CROWD_PARTICIPANTS` (other guests at work, default `0,10,20`), `CROWD_FRAMES` (300).
#[test]
#[ignore = "measuring tool: see context/mcp-remote/test-charge.md"]
fn crowd_guest_frames() {
    crate::tests_crowd_ui::run_guest();
}
