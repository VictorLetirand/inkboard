//! In-memory transport for tests: virtual clock, and latency, loss, duplication and reordering injected from a seed. Nothing here
//! waits or reads a real clock, so a run is fully reproducible.

use crate::crypto::SeededEntropy;
use crate::error::{Error, Result};
use crate::transport::{Connection, Transport};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// Faults to inject. Probabilities are in parts per million.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NetConfig {
    /// Each frame is delayed by a value in this range (milliseconds); a wide range reorders frames.
    pub latency_ms: (u64, u64),
    pub loss_ppm: u32,
    pub dup_ppm: u32,
    /// Chance that a frame is delayed by up to `reorder_extra_ms` more.
    pub reorder_ppm: u32,
    pub reorder_extra_ms: u64,
}

impl NetConfig {
    /// Instant, reliable and ordered.
    pub fn ideal() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    pub sent: u64,
    pub lost: u64,
    pub duplicated: u64,
    pub delivered: u64,
}

/// (connection, is the host's end)
type End = (u64, bool);

struct Inner {
    now: u64,
    rng: SeededEntropy,
    cfg: NetConfig,
    tie: u64,
    next_conn: u64,
    in_flight: BTreeMap<(u64, u64), (End, Vec<u8>)>,
    inboxes: HashMap<End, VecDeque<Vec<u8>>>,
    open: HashSet<u64>,
    listeners: HashMap<String, VecDeque<SimConn>>,
    stats: NetStats,
}

impl Inner {
    fn roll(&mut self, ppm: u32) -> bool {
        ppm > 0 && self.rng.next_u64() % 1_000_000 < u64::from(ppm)
    }

    fn deliver_due(&mut self) {
        while let Some(entry) = self.in_flight.first_entry() {
            if entry.key().0 > self.now {
                break;
            }
            let (end, frame) = entry.remove();
            if self.open.contains(&end.0) {
                self.stats.delivered += 1;
                self.inboxes.entry(end).or_default().push_back(frame);
            }
        }
    }

    fn schedule(&mut self, to: End, frame: Vec<u8>) {
        let (lo, hi) = self.cfg.latency_ms;
        let mut delay = lo + self.rng.next_u64() % (hi.saturating_sub(lo) + 1);
        let reorder = self.cfg.reorder_ppm;
        if self.roll(reorder) {
            delay += self.rng.next_u64() % (self.cfg.reorder_extra_ms + 1);
        }
        self.tie += 1;
        self.in_flight.insert((self.now + delay, self.tie), (to, frame));
    }
}

/// The simulated network, shared by all the endpoints created from it.
#[derive(Clone)]
pub struct SimNet(Arc<Mutex<Inner>>);

impl fmt::Debug for SimNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let g = self.lock();
        f.debug_struct("SimNet").field("now", &g.now).field("in_flight", &g.in_flight.len()).finish_non_exhaustive()
    }
}

impl SimNet {
    pub fn new(seed: u64, cfg: NetConfig) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            now: 0,
            rng: SeededEntropy::new(seed),
            cfg,
            tie: 0,
            next_conn: 1,
            in_flight: BTreeMap::new(),
            inboxes: HashMap::new(),
            open: HashSet::new(),
            listeners: HashMap::new(),
            stats: NetStats::default(),
        })))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().expect("simulated network lock poisoned")
    }

    /// Virtual time in milliseconds.
    pub fn now(&self) -> u64 {
        self.lock().now
    }

    /// Moves the virtual clock forward and delivers what is due.
    pub fn advance(&self, ms: u64) {
        let mut g = self.lock();
        g.now += ms;
        g.deliver_due();
    }

    /// Jumps to the delivery time of the next frame in flight. Returns `false` when nothing is in flight.
    pub fn advance_to_next(&self) -> bool {
        let mut g = self.lock();
        let Some(&(t, _)) = g.in_flight.keys().next() else { return false };
        g.now = g.now.max(t);
        g.deliver_due();
        true
    }

    /// Frames sent and not yet received (in flight or waiting in an inbox).
    pub fn pending_frames(&self) -> usize {
        let g = self.lock();
        g.in_flight.len() + g.inboxes.values().map(VecDeque::len).sum::<usize>()
    }

    /// Changes the faults from now on (e.g. `NetConfig::ideal()` to let the network heal).
    pub fn set_config(&self, cfg: NetConfig) {
        self.lock().cfg = cfg;
    }

    pub fn stats(&self) -> NetStats {
        self.lock().stats
    }

    /// Endpoint named `id` (a host registers under the identity put in its ticket).
    pub fn transport(&self, id: &str) -> SimTransport {
        self.lock().listeners.entry(id.to_owned()).or_default();
        SimTransport { net: self.clone(), me: id.to_owned() }
    }
}

#[derive(Debug)]
pub struct SimTransport {
    net: SimNet,
    me: String,
}

impl Transport for SimTransport {
    type Conn = SimConn;

    fn connect(&mut self, host: &str) -> Result<SimConn> {
        let mut g = self.net.lock();
        if !g.listeners.contains_key(host) {
            return Err(Error::Unexpected("no such host on the simulated network"));
        }
        let id = g.next_conn;
        g.next_conn += 1;
        g.open.insert(id);
        let host_end = SimConn { net: self.net.clone(), conn: id, host_end: true, peer: self.me.clone() };
        g.listeners.entry(host.to_owned()).or_default().push_back(host_end);
        Ok(SimConn { net: self.net.clone(), conn: id, host_end: false, peer: host.to_owned() })
    }

    fn accept(&mut self) -> Option<SimConn> {
        self.net.lock().listeners.get_mut(&self.me)?.pop_front()
    }
}

pub struct SimConn {
    net: SimNet,
    conn: u64,
    host_end: bool,
    peer: String,
}

impl fmt::Debug for SimConn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimConn").field("conn", &self.conn).field("peer", &self.peer).finish_non_exhaustive()
    }
}

impl SimConn {
    /// Number of this connection (the same on both ends).
    pub fn id(&self) -> u64 {
        self.conn
    }
}

impl Connection for SimConn {
    fn peer(&self) -> &str {
        &self.peer
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        let mut g = self.net.lock();
        if !g.open.contains(&self.conn) {
            return Err(Error::Closed);
        }
        g.stats.sent += 1;
        let (loss, dup) = (g.cfg.loss_ppm, g.cfg.dup_ppm);
        if g.roll(loss) {
            g.stats.lost += 1;
            return Ok(());
        }
        let to = (self.conn, !self.host_end);
        g.schedule(to, frame.to_vec());
        if g.roll(dup) {
            g.stats.duplicated += 1;
            g.schedule(to, frame.to_vec());
        }
        g.deliver_due();
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        let mut g = self.net.lock();
        g.deliver_due();
        g.inboxes.get_mut(&(self.conn, self.host_end))?.pop_front()
    }

    fn is_open(&self) -> bool {
        self.net.lock().open.contains(&self.conn)
    }

    fn close(&mut self) {
        self.net.lock().open.remove(&self.conn);
    }
}
