//! The clock of the simulation, the way a guest reaches the host, and a connection wrapper that makes the link bad on purpose
//! (latency, jitter, frame loss, a bandwidth cap on what is read, a guest that stops reading).

use crate::rng::Rng;
use inkboard_session::error::Result;
use inkboard_session::transport::Connection;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Time for the guests: a monotonic millisecond counter (protocol timers) and the wall clock in microseconds (markers that other
/// guests of the same machine read back to measure how long a modification took to become visible).
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
    fn epoch_us(&self) -> u64;
}

#[derive(Debug)]
pub struct RealClock {
    start: Instant,
}

impl Default for RealClock {
    fn default() -> Self {
        Self { start: Instant::now() }
    }
}

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn epoch_us(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
    }
}

/// Virtual time, moved by the test (deterministic runs on the simulated network).
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
    fn epoch_us(&self) -> u64 {
        self.0.load(Ordering::SeqCst) * 1000
    }
}

/// Opens connections to the host. A guest keeps one dialer for its whole stay so that it keeps the same identity when it reconnects.
pub trait Dialer: Send {
    /// May block for seconds (the guest calls it on a thread).
    fn dial(&mut self) -> std::result::Result<Box<dyn Connection + Send>, String>;

    /// `true` when `dial` never blocks (a simulated network): it is then called at once, on the caller's thread.
    fn instant(&self) -> bool {
        false
    }
}

/// What is wrong with a guest's link. All zeros: a perfect link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkSpec {
    /// Latency added to every frame, each way, and its random spread.
    pub delay_ms: u64,
    pub jitter_ms: u64,
    /// Probability that an incoming frame is dropped after it was read (the protocol notices the gap and asks again).
    pub loss: f64,
    /// Inbound bandwidth, kbit/s (0: unlimited). The wrapper stops reading when the budget is spent, so the pressure reaches the host.
    pub kbps: u64,
}

impl LinkSpec {
    #[must_use]
    pub fn is_perfect(&self) -> bool {
        *self == Self::default()
    }
}

/// Bytes read from / written to the transports by all the guests of the process (the timeline of the run).
pub static WIRE_IN: AtomicU64 = AtomicU64::new(0);
pub static WIRE_OUT: AtomicU64 = AtomicU64::new(0);

/// Counters shared with the guest (frames and bytes that crossed this connection).
#[derive(Debug, Default)]
pub struct LinkCounters {
    pub frames_in: AtomicU64,
    pub bytes_in: AtomicU64,
    pub frames_out: AtomicU64,
    pub bytes_out: AtomicU64,
    pub dropped: AtomicU64,
}

#[allow(missing_debug_implementations)]
pub struct Impaired {
    inner: Box<dyn Connection + Send>,
    spec: LinkSpec,
    rng: Rng,
    clock: Arc<dyn Clock>,
    inq: VecDeque<(u64, Vec<u8>)>,
    outq: VecDeque<(u64, Vec<u8>)>,
    last_in: u64,
    last_out: u64,
    /// Bytes that may still be read (can go negative: a frame is read whole).
    credit: f64,
    last_credit_ms: u64,
    /// While set, nothing is read at all.
    pub frozen: Arc<AtomicBool>,
    pub counters: Arc<LinkCounters>,
}

impl Impaired {
    pub fn new(inner: Box<dyn Connection + Send>, spec: LinkSpec, seed: u64, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now_ms();
        Self {
            inner,
            spec,
            rng: Rng::new(seed),
            clock,
            inq: VecDeque::new(),
            outq: VecDeque::new(),
            last_in: 0,
            last_out: 0,
            credit: 0.0,
            last_credit_ms: now,
            frozen: Arc::new(AtomicBool::new(false)),
            counters: Arc::new(LinkCounters::default()),
        }
    }

    fn lag(&mut self) -> u64 {
        self.spec.delay_ms + if self.spec.jitter_ms > 0 { self.rng.below(self.spec.jitter_ms as usize + 1) as u64 } else { 0 }
    }

    fn pump(&mut self) {
        let now = self.clock.now_ms();
        while self.outq.front().is_some_and(|(at, _)| *at <= now) {
            if let Some((_, f)) = self.outq.pop_front() {
                let _ = self.inner.send(&f);
            }
        }
        if self.frozen.load(Ordering::Relaxed) {
            self.last_credit_ms = now;
            return;
        }
        if self.spec.kbps > 0 {
            let earned = now.saturating_sub(self.last_credit_ms) as f64 * self.spec.kbps as f64 / 8.0; // kbit/s = bytes per ms / 8 * ... (1 kbit = 125 bytes)
            self.credit = (self.credit + earned).min(self.spec.kbps as f64 * 125.0); // at most one second of credit
            self.last_credit_ms = now;
            if self.credit < 0.0 {
                return;
            }
        }
        // read what the budget allows; the rest stays in the transport (that is the point: its queue grows)
        let mut taken = 0;
        while taken < 4096 && (self.spec.kbps == 0 || self.credit >= 0.0) {
            let Some(f) = self.inner.recv() else { break };
            taken += 1;
            self.counters.frames_in.fetch_add(1, Ordering::Relaxed);
            self.counters.bytes_in.fetch_add(f.len() as u64, Ordering::Relaxed);
            WIRE_IN.fetch_add(f.len() as u64, Ordering::Relaxed);
            self.credit -= f.len() as f64;
            if self.spec.loss > 0.0 && self.rng.chance(self.spec.loss) {
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let at = (now + self.lag()).max(self.last_in);
            self.last_in = at;
            self.inq.push_back((at, f));
        }
    }
}

impl Connection for Impaired {
    fn peer(&self) -> &str {
        self.inner.peer()
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.counters.frames_out.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes_out.fetch_add(frame.len() as u64, Ordering::Relaxed);
        WIRE_OUT.fetch_add(frame.len() as u64, Ordering::Relaxed);
        if self.spec.delay_ms == 0 && self.spec.jitter_ms == 0 && self.outq.is_empty() {
            return self.inner.send(frame);
        }
        let at = (self.clock.now_ms() + self.lag()).max(self.last_out);
        self.last_out = at;
        self.outq.push_back((at, frame.to_vec()));
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.pump();
        let now = self.clock.now_ms();
        if self.inq.front().is_some_and(|(at, _)| *at <= now) { self.inq.pop_front().map(|(_, f)| f) } else { None }
    }

    fn is_open(&self) -> bool {
        // frames already read stay readable after the transport closed
        self.inner.is_open() || !self.inq.is_empty()
    }

    fn close(&mut self) {
        self.inner.close();
    }
}
