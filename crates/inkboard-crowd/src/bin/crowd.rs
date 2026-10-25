//! `crowd`: the load-test tool of shared sessions.
//!
//! ```text
//! crowd guests  --ticket-file F [--view-ticket-file F] --guests N --mode loopback|lan|relay --duration S --settle S --out DIR ...
//! crowd run     --host-exe PATH --guests N --objects N --duration S --out DIR ...     (host + guests, local, two processes)
//! crowd report  DIR                                                                   (convergence, tables for one run)
//! crowd matrix  DIR...                                                                (one table line per run)
//! crowd board   --objects N                                                           (size of a seed board)
//! ```
//! The host is the `crowd_host` test of `inkboard-app` (`--host-exe` is its test executable). Options of the profile of the simulated
//! people: `--profile default|calm|busy`, `--cursor-hz`, `--strokes-per-min`, `--slow 0.1`, `--drops-per-hour`... (see `Profile::apply`).
//! Method and results: `context/mcp-remote/test-charge.md`.

use inkboard_crowd::board::{BoardSpec, asset_bytes, object_count, seed_document};
use inkboard_crowd::fleet::{FleetConfig, run_fleet};
use inkboard_crowd::guest::GuestEnv;
use inkboard_crowd::link::{Dialer, RealClock};
use inkboard_crowd::net::{IrohDialer, Mode};
use inkboard_crowd::profile::{Args, Profile};
use inkboard_crowd::report::{FleetReport, GuestReport, HostReport};
use inkboard_crowd::stats::Summary;
use inkboard_session::crypto::Link;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args = Args::new(std::env::args().skip(1));
    let result = match args.words.first().map(String::as_str) {
        Some("guests") => cmd_guests(&args),
        Some("run") => cmd_run(&args),
        Some("report") => args.words.get(1).map(PathBuf::from).ok_or_else(|| "report DIR".to_owned()).and_then(|d| cmd_report(&d)),
        Some("matrix") => cmd_matrix(&args),
        Some("board") => cmd_board(&args),
        Some("load") => {
            let mut s = inkboard_crowd::proc::ProcSampler::new();
            std::thread::sleep(Duration::from_secs(3));
            println!("{:.0}", s.sample().machine_cpu_pct);
            Ok(())
        }
        _ => Err("usage: crowd guests|run|report|matrix|board (see the header of crates/inkboard-crowd/src/bin/crowd.rs)".to_owned()),
    };
    if let Err(e) = result {
        eprintln!("crowd: {e}");
        std::process::exit(2);
    }
}

fn profile(args: &Args) -> Result<Profile, String> {
    let mut p = match args.get("profile") {
        Some(name) => Profile::named(name).ok_or_else(|| format!("unknown profile '{name}' (default, calm, busy)"))?,
        None => Profile::default(),
    };
    p.apply(args)?;
    Ok(p)
}

fn read_link(args: &Args, text: &str, file: &str) -> Result<Option<Link>, String> {
    let raw = match (args.get(text), args.get(file)) {
        (Some(t), _) => t.to_owned(),
        (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
        (None, None) => return Ok(None),
    };
    Link::parse(&raw).map(Some).map_err(|e| format!("link: {e}"))
}

/* ------------------------------------------------------------------------------------------------------------------ guests */

fn cmd_guests(a: &Args) -> Result<(), String> {
    let edit = read_link(a, "ticket", "ticket-file")?.ok_or("--ticket or --ticket-file is required")?;
    let view = read_link(a, "view-ticket", "view-ticket-file")?;
    let mode = Mode::parse(a.get("mode").unwrap_or("loopback")).ok_or("--mode: loopback, lan or relay")?;
    let guests: usize = a.or("guests", 10)?;
    let machine: u16 = a.or("machine", 1)?;
    let t0 = match (a.parse::<u64>("t0")?, a.get("t0-file")) {
        (Some(t), _) => Some(t),
        (None, Some(f)) => std::fs::read_to_string(f).ok().and_then(|s| s.trim().parse().ok()),
        _ => None,
    };
    let cfg = FleetConfig {
        guests,
        first_index: a.or("first-index", 0)?,
        threads: a.or("threads", (guests / 10).clamp(2, 8))?,
        machine_name: a.get("name").map_or_else(|| format!("m{machine}"), str::to_owned),
        t0_epoch_ms: t0,
        duration_s: a.or("duration", 60.0)?,
        settle_s: a.or("settle", 10.0)?,
        out_dir: a.get("out").map(PathBuf::from),
        raw: a.flag("raw"),
        transport: mode.name().to_owned(),
    };
    let env = Arc::new(GuestEnv {
        clock: Arc::new(RealClock::default()),
        profile: profile(a)?,
        machine,
        edit_ticket: edit.ticket.clone(),
        view_ticket: view.map(|v| v.ticket),
        deterministic: false,
        seed: a.or("seed", 1)?,
    });
    let ticket = edit.ticket;
    let make: Arc<dyn Fn(usize) -> Box<dyn Dialer> + Send + Sync> = Arc::new(move |_| {
        // a failure to build the endpoint is reported by the first dial
        match IrohDialer::new(mode, ticket.clone()) {
            Ok(d) => Box::new(d) as Box<dyn Dialer>,
            Err(e) => Box::new(Failing(e)),
        }
    });
    let report = run_fleet(&cfg, env, make);
    eprintln!(
        "[crowd {}] done: {} guests, {} joined, rtt p50/p95/p99 {} ms, snapshot p50/p95 {:.0}/{:.0} ms, peak rss {:.0} MiB",
        cfg.machine_name,
        report.guests,
        report.joined,
        report.rtt_ms.triple(),
        report.snapshot_ms.p50,
        report.snapshot_ms.p95,
        report.peak_rss_mib
    );
    Ok(())
}

struct Failing(String);

impl Dialer for Failing {
    fn dial(&mut self) -> Result<Box<dyn inkboard_session::transport::Connection + Send>, String> {
        Err(self.0.clone())
    }
}

/* ---------------------------------------------------------------------------------------------------------------------- run */

/// Waits until the computer is not busy with something else (a measurement taken while other programs compile means little).
fn wait_for_quiet(max_pct: f64, max_wait: Duration) {
    let mut sampler = inkboard_crowd::proc::ProcSampler::new();
    let started = Instant::now();
    let mut calm = 0;
    while started.elapsed() < max_wait {
        std::thread::sleep(Duration::from_secs(2));
        let load = sampler.sample().machine_cpu_pct;
        calm = if load <= max_pct { calm + 1 } else { 0 };
        if calm >= 3 {
            return;
        }
        eprintln!("[crowd] the computer is busy ({load:.0} %), waiting for it to calm down below {max_pct:.0} %...");
    }
    eprintln!("[crowd] still busy after {:.0} s: measuring anyway (the load of the computer is recorded in the report)", started.elapsed().as_secs_f64());
}

fn cmd_run(a: &Args) -> Result<(), String> {
    if let Some(p) = a.parse::<f64>("wait-idle")? {
        wait_for_quiet(p, Duration::from_secs(a.or("wait-max", 1800)?));
    }
    let exe = a.get("host-exe").ok_or("--host-exe PATH (the test executable of inkboard-app) is required")?.to_owned();
    let out = PathBuf::from(a.get("out").ok_or("--out DIR is required")?);
    let guests: usize = a.or("guests", 10)?;
    // `--procs 0 --external 1`: no local guests, one process of guests elsewhere (see scripts/crowd-remote.sh)
    let procs: usize = a.or("procs", 1)?;
    let duration: f64 = a.or("duration", 60.0)?;
    let settle: f64 = a.or("settle", 10.0)?;
    let mode = a.get("mode").unwrap_or("loopback").to_owned();
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut host = Command::new(&exe);
    host.args(["tests_crowd::crowd_host", "--ignored", "--exact", "--nocapture"])
        .env("CROWD_OUT", &out)
        .env("CROWD_OBJECTS", a.get("objects").unwrap_or("1000"))
        .env("CROWD_SEED", a.get("seed").unwrap_or("1"))
        .env("CROWD_NET", &mode)
        .env("CROWD_MAX", (guests + 10).to_string())
        .env("CROWD_DURATION", duration.to_string())
        .env("CROWD_SETTLE", settle.to_string())
        .env("CROWD_WARMUP", a.get("warmup").unwrap_or("8"))
        .env("CROWD_HOST_ACTIVITY", if a.flag("no-host-activity") { "0" } else { "1" })
        .env("CROWD_APP", if a.flag("app") { "1" } else { "0" });
    if let Some(p) = a.get("pending") {
        host.env("CROWD_PENDING", p);
    }
    let log = std::fs::File::create(out.join("host.log")).map_err(|e| e.to_string())?;
    host.stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?)).stderr(Stdio::from(log));
    let mut host = host.spawn().map_err(|e| format!("{exe}: {e}"))?;
    let waited = Instant::now();
    while !out.join("edit.link").exists() {
        if let Ok(Some(st)) = host.try_wait() {
            return Err(format!("the host stopped early ({st}); see {}", out.join("host.log").display()));
        }
        if waited.elapsed() > Duration::from_secs(600) {
            let _ = host.kill();
            return Err("the host did not become ready in 10 minutes".to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let t0 = std::fs::read_to_string(out.join("t0")).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut children = Vec::new();
    let mut first = 0usize;
    for p in 0..procs {
        let n = guests / procs + usize::from(p < guests % procs);
        let mut c = Command::new(&me);
        c.arg("guests")
            .args(["--ticket-file"])
            .arg(out.join("edit.link"))
            .arg("--view-ticket-file")
            .arg(out.join("view.link"))
            .args([
                "--t0",
                t0.trim(),
                "--guests",
                &n.to_string(),
                "--first-index",
                &first.to_string(),
                "--name",
                &format!("p{p}"),
                "--machine",
                "1",
                "--mode",
                &mode,
            ])
            .args(["--duration", &duration.to_string(), "--settle", &settle.to_string(), "--out"])
            .arg(&out)
            .args(["--seed", a.get("seed").unwrap_or("1")]);
        // every option of the profile is passed on as it came
        for key in PROFILE_OPTIONS {
            if let Some(v) = a.get(key) {
                c.arg(format!("--{key}={v}"));
            }
        }
        if let Some(p) = a.get("profile") {
            c.arg(format!("--profile={p}"));
        }
        if a.flag("no-images") {
            c.arg("--no-images");
        }
        let log = std::fs::File::create(out.join(format!("guests-p{p}.log"))).map_err(|e| e.to_string())?;
        c.stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?)).stderr(Stdio::from(log));
        children.push(c.spawn().map_err(|e| e.to_string())?);
        first += n;
    }
    for mut c in children {
        let _ = c.wait();
    }
    // guests started elsewhere (another machine): wait for their reports to be copied into the folder
    let external: usize = a.or("external", 0)?;
    if external > 0 {
        eprintln!("[crowd] host ready: t0={}, links in {}; waiting for {external} report(s) guests-*.json in that folder", t0.trim(), out.display());
        let limit = Instant::now() + Duration::from_secs_f64(a.or("duration", 60.0)? + a.or("settle", 10.0)? + 600.0);
        let count = || {
            std::fs::read_dir(&out).map_or(0, |d| {
                d.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with("guests-") && e.file_name().to_string_lossy().ends_with(".json"))
                    .count()
            })
        };
        while count() < procs + external && Instant::now() < limit {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    std::fs::write(out.join("stop"), b"stop").map_err(|e| e.to_string())?;
    let waited = Instant::now();
    while host.try_wait().ok().flatten().is_none() {
        if waited.elapsed() > Duration::from_secs(120) {
            let _ = host.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    cmd_report(&out)
}

const PROFILE_OPTIONS: &[&str] = &[
    "cursor-hz",
    "cursor-active",
    "strokes-per-min",
    "creates-per-min",
    "moves-per-min",
    "deletes-per-min",
    "texts-per-min",
    "reorders-per-min",
    "undos-per-min",
    "images-per-min",
    "big-image-prob",
    "viewers",
    "slow",
    "slow-loss",
    "stuck",
    "stuck-after",
    "mean-stay",
    "drops-per-hour",
    "ramp",
    "slow-delay-ms",
    "slow-jitter-ms",
    "slow-kbps",
];

/* ------------------------------------------------------------------------------------------------------------------ report */

/// What one run says: the host, every process of guests, and whether everybody holds the host's board.
struct Run {
    host: Option<HostReport>,
    guests: Vec<FleetReport>,
}

fn load(dir: &Path) -> Result<Run, String> {
    let host = std::fs::read(dir.join("host.json")).ok().and_then(|b| serde_json::from_slice::<HostReport>(&b).ok());
    let mut guests = Vec::new();
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("guests-") && n.ends_with(".json")))
        .collect();
    names.sort();
    for p in names {
        guests.push(serde_json::from_slice::<FleetReport>(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", p.display()))?);
    }
    Ok(Run { host, guests })
}

impl Run {
    fn all(&self) -> impl Iterator<Item = &GuestReport> {
        self.guests.iter().flat_map(|f| f.per_guest.iter())
    }

    /// (converged, diverged, away). "Absent": a guest that ended gone (refused, removed, never in), or on a degraded link that is
    /// still receiving (no board yet, or behind the host's last modification): it is late, not wrong. "Diverged" is a guest that
    /// has caught up with the host's last modification (or is on a good link) and holds another board: a bug.
    fn convergence(&self) -> (usize, usize, usize) {
        let (digest, seq) = self.host.as_ref().map(|h| (h.digest.as_str(), h.seq_final)).unwrap_or_default();
        let (mut ok, mut bad, mut away) = (0, 0, 0);
        for g in self.all() {
            let in_session = g.joins > 0 && !g.digest.is_empty() && g.end != "left" && !g.end.starts_with("closed") && !g.end.starts_with("refused");
            let late = !g.link.is_perfect() && (g.objects == 0 || g.last_seq < seq);
            if !in_session || (late && g.digest != digest) {
                away += 1;
            } else if g.digest == digest && g.pending == 0 {
                ok += 1;
            } else {
                bad += 1;
            }
        }
        (ok, bad, away)
    }

    fn merged(&self, f: impl Fn(&FleetReport) -> Summary, weight: impl Fn(&FleetReport) -> usize) -> Summary {
        // the percentiles of several processes cannot be merged exactly; the worst process is shown (and the weighted median)
        let mut best = Summary::default();
        let mut tot = 0usize;
        let mut p50 = 0.0;
        for r in &self.guests {
            let s = f(r);
            if s.n == 0 {
                continue;
            }
            let w = weight(r).max(1);
            p50 += s.p50 * w as f64;
            tot += w;
            best.n += s.n;
            best.p95 = best.p95.max(s.p95);
            best.p99 = best.p99.max(s.p99);
            best.max = best.max.max(s.max);
            best.min = if best.min == 0.0 { s.min } else { best.min.min(s.min) };
        }
        if tot > 0 {
            best.p50 = p50 / tot as f64;
        }
        best
    }
}

fn row(dir: &Path, run: &Run) -> String {
    let (ok, bad, away) = run.convergence();
    let total = ok + bad + away;
    let h = run.host.clone().unwrap_or_default();
    let rtt = run.merged(|r| r.rtt_ms, |r| r.edits_confirmed as usize);
    let vis = run.merged(|r| r.visible_ms, |r| r.visible_ms.n);
    let snap = run.merged(|r| r.snapshot_ms, |r| r.joined);
    let img = run.merged(|r| r.images_ms, |r| r.joined);
    let cur = run.merged(|r| r.cursor_ms, |r| r.cursor_ms.n);
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    format!(
        "| {name} | {} | {} | {:.0} / {:.0} / {:.0} | {:.0} / {:.0} | {:.0} / {:.0} / {:.0} | {:.0} / {:.0} / {:.0} | {:.0} / {:.0} / {:.0} | {:.2} / {:.2} | {:.0} | {:.0} / {:.0} | {:.1} / {:.1} | {:.1} / {:.1} | {:.0} | {:.1} / {:.0} | {} / {} | {ok}/{total}{} |",
        h.board_objects,
        h.members_peak,
        snap.p50,
        snap.p95,
        snap.max,
        img.p50,
        img.p95,
        rtt.p50,
        rtt.p95,
        rtt.p99,
        vis.p50,
        vis.p95,
        vis.p99,
        cur.p50,
        cur.p95,
        cur.p99,
        h.cpu_cores.p50,
        h.peak_cpu_cores,
        h.peak_rss_mib,
        h.machine_cpu_pct.p50,
        h.machine_cpu_pct.p95,
        h.tx_mib_s.p50,
        h.tx_mib_s.max,
        h.rx_mib_s.p50,
        h.rx_mib_s.max,
        h.queue_kib_worst.max,
        h.turn_ms.p99,
        h.turn_ms.max,
        h.warned,
        h.excluded,
        if bad > 0 { format!(" (**{bad} diverged**)") } else { String::new() },
    )
}

const HEADER: &str = "| run | objects | participants (peak) | snapshot ms p50 / p95 / max | images ms p50 / p95 | edit round trip ms p50 / p95 / p99 | visible to others ms p50 / p95 / p99 | cursor ms p50 / p95 / p99 | host CPU cores p50 / peak | host RSS peak MiB | machine CPU % p50 / p95 | host tx MiB/s p50 / max | host rx MiB/s p50 / max | worst queue KiB | host turn ms p99 / max | warned / excluded | converged |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|";

fn cmd_report(dir: &Path) -> Result<(), String> {
    let run = load(dir)?;
    let (ok, bad, away) = run.convergence();
    let mut md = format!("# Crowd run `{}`\n\n{HEADER}\n{}\n\n", dir.display(), row(dir, &run));
    md += &format!(
        "Convergence: {ok} guests hold the host's board (same digest, nothing pending), {bad} diverged, {away} are not in the session at the end (left, removed, never joined) or are late (degraded link, still receiving).\n\n"
    );
    if let Some(h) = &run.host {
        md += &format!(
            "Host: {} mode over {}; board {} objects on {} pages, {:.1} MiB of JSON, {} images ({:.1} MiB); turn p50/p95/p99/max {:.2}/{:.2}/{:.2}/{:.2} ms, turns over 16 ms: {:.1} %, over 50 ms: {:.1} %; \
             tx {:.1} MiB ({} frames), rx {:.1} MiB ({} frames); log {} entries ({:.0} KiB), {} objects tracked, final seq {}; send errors: {:?}; warnings: {:?}; exclusions: {:?}.\n\n",
            h.mode,
            h.transport,
            h.board_objects,
            h.board_pages,
            h.board_json_mib,
            h.board_images,
            h.board_asset_mib,
            h.turn_ms.p50,
            h.turn_ms.p95,
            h.turn_ms.p99,
            h.turn_ms.max,
            h.turn_over_16_pct,
            h.turn_over_50_pct,
            h.tx_mib,
            h.frames_tx,
            h.rx_mib,
            h.frames_rx,
            h.log_entries,
            h.log_kib,
            h.tracked_objects,
            h.seq_final,
            h.send_errors,
            h.warnings,
            h.excluded_reasons
        );
    }
    for f in &run.guests {
        md += &format!(
            "Guests `{}` ({}): {} guests, {} joined, {} never joined, dial p50/p95 {:.0}/{:.0} ms, welcome p50/p95 {:.0}/{:.0} ms, snapshot {:.0} KiB p50, edits {} sent / {} confirmed, drops {} (resume p50/p95 {:.0}/{:.0} ms, {} by snapshot), \
             rejected {:?}, peak rss {:.0} MiB, peak cpu {:.2} cores.\n\n",
            f.machine,
            f.transport,
            f.guests,
            f.joined,
            f.never_joined,
            f.dial_ms.p50,
            f.dial_ms.p95,
            f.welcome_ms.p50,
            f.welcome_ms.p95,
            f.snapshot_kib.p50,
            f.edits_sent,
            f.edits_confirmed,
            f.drops,
            f.resume_ms.p50,
            f.resume_ms.p95,
            f.resume_snapshots,
            f.rejected,
            f.peak_rss_mib,
            f.peak_cpu_cores
        );
    }
    let bad_guests: Vec<String> = run
        .all()
        .filter(|g| {
            g.joins > 0 && g.digest != run.host.as_ref().map(|h| h.digest.clone()).unwrap_or_default() && !g.end.starts_with("closed") && g.end != "left"
        })
        .map(|g| format!("{} (objects {}, pending {}, seq {}, end {})", g.name, g.objects, g.pending, g.last_seq, g.end))
        .collect();
    if !bad_guests.is_empty() {
        md += &format!("Guests that differ from the host: {}\n\n", bad_guests.join("; "));
    }
    let failures: Vec<String> = run.all().filter(|g| !g.dial_errors.is_empty()).map(|g| format!("{}: {}", g.name, g.dial_errors[0])).take(5).collect();
    if !failures.is_empty() {
        md += &format!("Dial errors (first five): {}\n\n", failures.join("; "));
    }
    std::fs::write(dir.join("summary.md"), &md).map_err(|e| e.to_string())?;
    println!("{md}");
    Ok(())
}

fn cmd_matrix(a: &Args) -> Result<(), String> {
    println!("{HEADER}");
    for d in a.words.iter().skip(1) {
        let dir = PathBuf::from(d);
        println!("{}", row(&dir, &load(&dir)?));
    }
    Ok(())
}

/* ------------------------------------------------------------------------------------------------------------------- board */

fn cmd_board(a: &Args) -> Result<(), String> {
    let objects: usize = a.or("objects", 5_000)?;
    let spec = BoardSpec::sized(objects, a.or("seed", 1)?);
    let started = Instant::now();
    let doc = seed_document(&spec);
    let json = serde_json::to_vec(&doc.pages).map_err(|e| e.to_string())?.len();
    println!(
        "{} objects on {} pages, {:.1} MiB of JSON (the snapshot), {} images, {:.1} MiB of image bytes; built in {:.2} s",
        object_count(&doc),
        doc.pages.len(),
        json as f64 / 1_048_576.0,
        doc.assets.len(),
        asset_bytes(&doc) as f64 / 1_048_576.0,
        started.elapsed().as_secs_f64()
    );
    // what actually travels: the snapshot cut in parts, packed (protocol v3)
    let started = Instant::now();
    let snap = inkboard_session::proto::Snapshot::new(&doc, 1, Vec::new());
    let parts = inkboard_session::snapshot::into_messages(snap, inkboard_session::snapshot::PART_BYTES);
    let (mut plain, mut packed) = (0usize, 0usize);
    for m in &parts {
        let j = serde_json::to_vec(m).map_err(|e| e.to_string())?;
        plain += j.len();
        packed += match inkboard_session::pack::pack(m) {
            Some(p) => serde_json::to_vec(&p).map_err(|e| e.to_string())?.len(),
            None => j.len(),
        };
    }
    println!(
        "snapshot: {} parts, {:.1} MiB plain, {:.1} MiB packed (x{:.1}); cut and packed in {:.2} s",
        parts.len(),
        plain as f64 / 1_048_576.0,
        packed as f64 / 1_048_576.0,
        plain as f64 / packed.max(1) as f64,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
