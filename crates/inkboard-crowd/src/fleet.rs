//! A fleet: many simulated guests of one process, driven by a few worker threads, with arrivals spread over a ramp, optional leaving and
//! coming back, a quiet period at the end (so that everything settles) and a report.

use crate::guest::{GuestEnv, GuestPlan, SimGuest};
use crate::link::{Dialer, LinkSpec};
use crate::proc::ProcSampler;
use crate::report::{FleetReport, GuestReport};
use crate::rng::Rng;
use crate::stats::Samples;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct FleetConfig {
    pub guests: usize,
    /// Number of the first guest of this process (several processes share one numbering).
    pub first_index: usize,
    pub threads: usize,
    pub machine_name: String,
    /// Absolute start (epoch ms): the processes of a run line up on it. `None`: now.
    pub t0_epoch_ms: Option<u64>,
    pub duration_s: f64,
    pub settle_s: f64,
    pub out_dir: Option<PathBuf>,
    /// Write every latency sample (CSV).
    pub raw: bool,
    pub transport: String,
}

/// The shared state of the workers that the progress line and the timeline read.
#[derive(Default)]
struct Live {
    live: AtomicUsize,
    joined_ever: AtomicUsize,
    over: AtomicUsize,
}

/// Latency samples of one class of guests.
#[derive(Default)]
struct Lat {
    rtt: Samples,
    visible: Samples,
    cursor: Samples,
}

/// Samples of the guests on a perfect link and of those on a degraded one, apart (the second class would blur the first).
#[derive(Default)]
struct Buckets {
    fast: Lat,
    slow: Lat,
}

impl Buckets {
    fn merge(&mut self, o: &Buckets) {
        for (a, b) in [(&mut self.fast, &o.fast), (&mut self.slow, &o.slow)] {
            a.rtt.merge(&b.rtt);
            a.visible.merge(&b.visible);
            a.cursor.merge(&b.cursor);
        }
    }
}

struct Slot {
    id: usize,
    guest: Option<SimGuest>,
    /// Next arrival of somebody in this slot.
    arrive_at: u64,
    leave_at: Option<u64>,
    generation: u32,
    stuck: bool,
    finished: Vec<GuestReport>,
    done_samples: Buckets,
}

fn plan_for(env: &GuestEnv, id: usize, generation: u32, now: u64) -> GuestPlan {
    let p = &env.profile;
    let mut r = Rng::stream(env.seed, id as u64 * 1000 + u64::from(generation));
    let edit = env.view_ticket.is_none() || !r.chance(p.viewer_fraction);
    let slow = r.chance(p.slow_fraction);
    let link =
        if slow { LinkSpec { delay_ms: p.slow_delay_ms, jitter_ms: p.slow_jitter_ms, loss: p.slow_loss, kbps: p.slow_kbps } } else { LinkSpec::default() };
    let stuck = r.chance(p.stuck_fraction);
    GuestPlan {
        index: id,
        name: if generation == 0 { format!("G{id}") } else { format!("G{id}.{generation}") },
        edit,
        link,
        stuck_at_ms: stuck.then(|| now + (p.stuck_after_s * 1000.0) as u64),
    }
}

/// Runs the guests until the end of the run and returns the report. `make_dialer(index)` builds the way guest `index` reaches the host
/// (a new identity for each arrival).
pub fn run_fleet(cfg: &FleetConfig, env: Arc<GuestEnv>, make_dialer: Arc<dyn Fn(usize) -> Box<dyn Dialer> + Send + Sync>) -> FleetReport {
    let clock = Arc::clone(&env.clock);
    // line up on t0 (guests of other processes and the host use the same absolute time)
    let wait_ms = cfg.t0_epoch_ms.map_or(0, |t0| (t0 * 1000).saturating_sub(clock.epoch_us()) / 1000);
    let go = clock.now_ms() + wait_ms;
    let activity_end = go + (cfg.duration_s * 1000.0) as u64;
    let settle_end = activity_end + (cfg.settle_s * 1000.0) as u64;
    let profile = env.profile.clone();
    let threads = cfg.threads.clamp(1, cfg.guests.max(1));
    let live = Arc::new(Live::default());
    let (tx, rx) = channel::<(Vec<GuestReport>, Buckets)>();
    let quiet = Arc::new(AtomicBool::new(false));

    let mut workers = Vec::new();
    for w in 0..threads {
        let ids: Vec<usize> = (0..cfg.guests).filter(|i| i % threads == w).collect();
        let (env, live, quiet, tx, make) = (Arc::clone(&env), Arc::clone(&live), Arc::clone(&quiet), tx.clone(), Arc::clone(&make_dialer));
        let first = cfg.first_index;
        let ramp_ms = profile.ramp_s * 1000.0;
        let n = cfg.guests.max(1);
        let handle = std::thread::Builder::new().name(format!("crowd-worker-{w}")).spawn(move || {
            worker(&env, ids, first, go, activity_end, settle_end, ramp_ms, n, &live, &quiet, &tx, &*make);
        });
        workers.push(handle);
    }
    drop(tx);

    // main thread: progress line and per-second timeline
    let mut sampler = ProcSampler::new();
    let mut timeline = String::from("t_s,live,joined_ever,over,cpu_cores,rss_mib,machine_cpu_pct,wire_in_mib,wire_out_mib\n");
    let (mut peak_rss, mut peak_cpu) = (0.0f64, 0.0f64);
    let started = std::time::Instant::now();
    let mut last_progress = 0u64;
    loop {
        std::thread::sleep(Duration::from_millis(1000));
        let t = started.elapsed().as_secs_f64();
        let s = sampler.sample();
        peak_rss = peak_rss.max(s.rss_mib);
        peak_cpu = peak_cpu.max(s.cpu_cores);
        let _ = writeln!(
            timeline,
            "{t:.0},{},{},{},{:.2},{:.1},{:.1},{:.2},{:.2}",
            live.live.load(Ordering::Relaxed),
            live.joined_ever.load(Ordering::Relaxed),
            live.over.load(Ordering::Relaxed),
            s.cpu_cores,
            s.rss_mib,
            s.machine_cpu_pct,
            crate::link::WIRE_IN.load(Ordering::Relaxed) as f64 / 1_048_576.0,
            crate::link::WIRE_OUT.load(Ordering::Relaxed) as f64 / 1_048_576.0
        );
        if t as u64 >= last_progress + 5 {
            last_progress = t as u64;
            eprintln!(
                "[crowd {}] t={t:.0}s live={} joined={} over={} cpu={:.2} rss={:.0} MiB machine={:.0}%",
                cfg.machine_name,
                live.live.load(Ordering::Relaxed),
                live.joined_ever.load(Ordering::Relaxed),
                live.over.load(Ordering::Relaxed),
                s.cpu_cores,
                s.rss_mib,
                s.machine_cpu_pct
            );
        }
        if workers.iter().all(|w| w.as_ref().map_or(true, std::thread::JoinHandle::is_finished)) {
            break;
        }
    }
    let _ = quiet;
    let mut guests: Vec<GuestReport> = Vec::new();
    let mut lat = Buckets::default();
    for (reports, b) in rx.try_iter() {
        guests.extend(reports);
        lat.merge(&b);
    }
    for w in workers.into_iter().flatten() {
        let _ = w.join();
    }
    guests.sort_by(|a, b| (a.index, &a.name).cmp(&(b.index, &b.name)));
    let report = assemble(cfg, &env, guests, lat, peak_rss, peak_cpu);
    if let Some(dir) = &cfg.out_dir {
        let _ = std::fs::create_dir_all(dir);
        let m = &cfg.machine_name;
        let _ = std::fs::write(dir.join(format!("guests-{m}.json")), serde_json::to_vec_pretty(&report).unwrap_or_default());
        let _ = std::fs::write(dir.join(format!("guests-{m}-timeline.csv")), timeline);
    }
    report
}

#[allow(clippy::too_many_arguments)]
fn worker(
    env: &Arc<GuestEnv>,
    ids: Vec<usize>,
    first: usize,
    go: u64,
    activity_end: u64,
    settle_end: u64,
    ramp_ms: f64,
    total: usize,
    live: &Live,
    _quiet: &AtomicBool,
    tx: &Sender<(Vec<GuestReport>, Buckets)>,
    make: &(dyn Fn(usize) -> Box<dyn Dialer> + Send + Sync),
) {
    let clock = Arc::clone(&env.clock);
    let mut slots: Vec<Slot> = ids
        .iter()
        .map(|&i| {
            let mut r = Rng::stream(env.seed ^ 0xA77, (first + i) as u64);
            let arrive = go + (ramp_ms * (i as f64 + r.f64()) / total as f64) as u64;
            Slot {
                id: first + i,
                guest: None,
                arrive_at: arrive,
                leave_at: None,
                generation: 0,
                stuck: false,
                finished: Vec::new(),
                done_samples: Default::default(),
            }
        })
        .collect();
    let mut made_quiet = false;
    let mut counted_joined: Vec<bool> = vec![false; slots.len()];
    loop {
        let now = clock.now_ms();
        if now >= settle_end {
            break;
        }
        if now >= activity_end && !made_quiet {
            made_quiet = true;
            for s in &mut slots {
                if let Some(g) = s.guest.as_mut() {
                    g.set_quiet();
                }
            }
        }
        let mut busy = false;
        for (k, s) in slots.iter_mut().enumerate() {
            // arrival of a new person
            if s.guest.is_none() && now >= s.arrive_at && now < activity_end {
                let plan = plan_for(env, s.id, s.generation, now);
                s.stuck = plan.stuck_at_ms.is_some();
                let mut g = SimGuest::new(Arc::clone(env), plan, make(s.id * 1000 + s.generation as usize));
                g.start(now);
                s.leave_at = (env.profile.mean_stay_s > 0.0).then(|| {
                    let mut r = Rng::stream(env.seed ^ 0x1EAF, s.id as u64 * 1000 + u64::from(s.generation));
                    now + (r.exp(env.profile.mean_stay_s) * 1000.0) as u64
                });
                s.guest = Some(g);
                busy = true;
            }
            let Some(g) = s.guest.as_mut() else { continue };
            busy |= g.step(now);
            if !counted_joined[k] && g.rep.joins > 0 {
                counted_joined[k] = true;
                live.joined_ever.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(at) = s.leave_at
                && now >= at
                && g.is_live()
                && now < activity_end
            {
                g.leave(now);
                s.leave_at = None;
            }
            if g.is_over() && s.leave_at.is_none() && now < activity_end && env.profile.mean_stay_s > 0.0 {
                // somebody else comes later into this seat
                let mut done = s.guest.take().map(|mut g| g.finish(now)).expect("a guest");
                let mut r = Rng::stream(env.seed ^ 0x2B, s.id as u64 * 1000 + u64::from(s.generation));
                let (lo, hi) = env.profile.rejoin_pause_s;
                s.arrive_at = now + (r.range(lo, hi) * 1000.0) as u64;
                s.generation += 1;
                counted_joined[k] = false;
                absorb(&mut s.done_samples, &mut done);
                s.finished.push(done);
            }
        }
        live.live.store(slots.iter().filter(|s| s.guest.as_ref().is_some_and(SimGuest::is_live)).count(), Ordering::Relaxed);
        live.over.store(slots.iter().filter(|s| s.guest.as_ref().is_some_and(SimGuest::is_over)).count(), Ordering::Relaxed);
        if !busy {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let now = clock.now_ms();
    let mut reports = Vec::new();
    let mut samples = Buckets::default();
    for mut s in slots {
        if let Some(mut g) = s.guest.take() {
            let mut r = g.finish(now);
            absorb(&mut samples, &mut r);
            reports.push(r);
        }
        reports.extend(s.finished);
        samples.merge(&s.done_samples);
    }
    let _ = tx.send((reports, samples));
}

fn absorb(into: &mut Buckets, r: &mut GuestReport) {
    let b = if r.link.is_perfect() { &mut into.fast } else { &mut into.slow };
    b.rtt.merge(&r.rtt_ms);
    b.visible.merge(&r.visible_ms);
    b.cursor.merge(&r.cursor_ms);
    r.rtt_ms = Samples::default();
    r.visible_ms = Samples::default();
    r.cursor_ms = Samples::default();
}

#[allow(clippy::too_many_arguments)]
fn assemble(cfg: &FleetConfig, env: &GuestEnv, guests: Vec<GuestReport>, lat: Buckets, peak_rss: f64, peak_cpu: f64) -> FleetReport {
    let col = |f: &dyn Fn(&GuestReport) -> Option<f64>| Samples(guests.iter().filter(|g| g.link.is_perfect()).filter_map(f).collect()).summary();
    let slow_col = |f: &dyn Fn(&GuestReport) -> Option<f64>| Samples(guests.iter().filter(|g| !g.link.is_perfect()).filter_map(f).collect()).summary();
    let mut rejected = std::collections::BTreeMap::new();
    for g in &guests {
        for (k, v) in &g.rejected {
            *rejected.entry(k.clone()).or_insert(0) += v;
        }
    }
    let resume: Vec<f64> = guests.iter().flat_map(|g| g.resume_ms.iter().copied()).collect();
    FleetReport {
        machine: cfg.machine_name.clone(),
        seed: env.seed,
        guests: guests.len(),
        started_epoch_ms: env.clock.epoch_us() / 1000,
        duration_s: cfg.duration_s,
        profile: serde_json::to_value(&env.profile).unwrap_or_default(),
        transport: cfg.transport.clone(),
        rtt_ms: lat.fast.rtt.summary(),
        visible_ms: lat.fast.visible.summary(),
        cursor_ms: lat.fast.cursor.summary(),
        slow_rtt_ms: lat.slow.rtt.summary(),
        slow_visible_ms: lat.slow.visible.summary(),
        slow_cursor_ms: lat.slow.cursor.summary(),
        snapshot_ms: col(&|g| g.snapshot_ms),
        slow_snapshot_ms: slow_col(&|g| g.snapshot_ms),
        welcome_ms: col(&|g| g.welcome_ms),
        images_ms: col(&|g| g.images_ms),
        dial_ms: col(&|g| g.dial_ms),
        resume_ms: Samples(resume).summary(),
        snapshot_kib: col(&|g| (g.snapshot_kib > 0.0).then_some(g.snapshot_kib)),
        bytes_in: guests.iter().map(|g| g.bytes_in).sum(),
        bytes_out: guests.iter().map(|g| g.bytes_out).sum(),
        frames_in: guests.iter().map(|g| g.frames_in).sum(),
        frames_out: guests.iter().map(|g| g.frames_out).sum(),
        edits_sent: guests.iter().map(|g| g.edits_sent).sum(),
        edits_confirmed: guests.iter().map(|g| g.edits_confirmed).sum(),
        drops: guests.iter().map(|g| u64::from(g.drops)).sum(),
        resume_snapshots: guests.iter().map(|g| u64::from(g.resume_snapshots)).sum(),
        joined: guests.iter().filter(|g| g.joins > 0).count(),
        never_joined: guests.iter().filter(|g| g.joins == 0).count(),
        closed_by_host: guests.iter().filter(|g| g.closed.as_deref().is_some_and(|c| c.starts_with("closed") || c.starts_with("refused"))).count(),
        converged: 0,
        diverged: 0,
        rejected,
        peak_rss_mib: peak_rss,
        peak_cpu_cores: peak_cpu,
        per_guest: guests,
    }
}
