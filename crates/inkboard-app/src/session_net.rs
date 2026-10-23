//! The real network of shared sessions (lot I): `inkboard-net` (iroh) plugged into the host (`session_host`) and the guest
//! (`session_guest`).
//!
//! Nothing here runs until the user acts: the objects built at startup ([`host_net`], [`guest_factory`]) only hold a configuration.
//! The first socket opens in `SessionNet::start` ("Start sharing") or when the guest factory is called ("Join").
//!
//! - Host: [`IrohHostNet`] binds the endpoint, waits a few seconds for the relay and the local addresses to put them in the links,
//!   then keeps improving the address while the session runs (`refresh`). Dropping the transport (stop sharing) closes the endpoint
//!   after the final `Bye` was delivered and stops the network threads.
//! - Guest: [`GuestNet`] builds the session. The handshake runs on a thread (the interface never waits for the network); the guest
//!   keeps one iroh endpoint for the whole session, so the host recognizes it when it reconnects and gives its place back.
//! - Lot R: the network coming back is noticed (`LocalAddrWatch`) and iroh is told (`network_change`); the guest retries at once with
//!   the same endpoint (same identity). Closing a transport happens on a thread (`close_in_background`). The custom relays come from
//!   the `relays` file of the data folder (`parse_relay_list`).
//! - Failures: the transport's `Error::Network` becomes a [`ConnectError`] for the classification of `session_guest`; the relay is
//!   diagnosed on a thread with `inkboard_net::diagnose_relay`.

use crate::i18n::{t, tr};
use crate::join_ui::SessionFactory;
use crate::session_guest::{Clock, ConnectError, ConnectErrorKind, Diagnosis, GuestConfig, GuestLink, GuestSession, NetWatch, RelayDiagnoser};
use crate::session_host::{HostAddress, HostTransport, SessionNet, StartJob, StartResult};
use crate::session_link::{Dialing, connect_error};
use inkboard_net::diagnose::{DiagnoseOptions, diagnose_relay_with};
use inkboard_net::ticket::MAX_HINTS;
use inkboard_net::{CancelToken, GatewayKind, IrohConn, IrohTransport, NetConfig, RelaySetting};
use inkboard_session::crypto::{OsEntropy, Ticket};
use inkboard_session::transport::{Connection, Transport};
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long "Start sharing" waits for the relay and the local addresses (on a thread: the interface stays usable and the start can be
/// cancelled). What arrives later is added to the links by `refresh`.
const HOST_READY: Duration = Duration::from_secs(3);
/// A guest gives up on a handshake after this long (shorter than the session's own 20 s, so that the network says why).
const GUEST_CONNECT: Duration = Duration::from_secs(15);
/// How long the diagnosis of a relay may take.
const DIAGNOSE: Duration = Duration::from_secs(5);
/// The host's address is looked at again this often.
const REFRESH_EVERY: Duration = Duration::from_secs(1);

/* ---------------------------------------------------------------------------------------------------- closing in the background */

/// Threads that close network objects (the last frames are delivered first, which can take seconds when a peer is gone): the
/// interface never waits for them. `wait_background_closes` gives them a last chance when the application quits.
static CLOSING: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// Runs `job` (it drops a transport) on its own thread.
fn close_in_background(job: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new().name("inkboard-close".to_owned()).spawn(job);
    let mut list = CLOSING.lock().unwrap_or_else(PoisonError::into_inner);
    list.retain(|h| !h.is_finished());
    list.extend(handle.ok());
}

/// Tests: how many background closings are still running.
#[cfg(test)]
pub fn background_closes_pending() -> usize {
    CLOSING.lock().unwrap_or_else(PoisonError::into_inner).iter().filter(|h| !h.is_finished()).count()
}

/// At exit: waits for the closings in progress, at most `max` in all (a peer that does not answer must not keep the window open).
pub fn wait_background_closes(max: Duration) {
    let list = std::mem::take(&mut *CLOSING.lock().unwrap_or_else(PoisonError::into_inner));
    let deadline = Instant::now() + max;
    for h in list {
        while !h.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/* ------------------------------------------------------------------------------------------------------------ relay list */

/// File of the data folder with the custom relays, one `https://…` address per line (empty or absent: the public relays).
pub const RELAYS_FILE: &str = "relays";
/// A link carries at most 8 hints in all: a longer custom list would crowd out the host's own addresses.
pub const MAX_RELAYS: usize = 4;

/// Why a list of relays is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayListError {
    /// The line does not start with `https://`.
    NotHttps(String),
    /// The line starts with `https://` but is not an address.
    Invalid(String),
    TooMany,
}

impl RelayListError {
    pub fn message(&self) -> String {
        match self {
            RelayListError::NotHttps(l) => tr!("Not an https:// address: {}", l),
            RelayListError::Invalid(l) => tr!("Not a valid relay address: {}", l),
            RelayListError::TooMany => tr!("At most {} relays.", MAX_RELAYS),
        }
    }
}

/// Reads a list typed by a person: one address per line, `https://` only, empty lines and repeats ignored.
pub fn parse_relay_list(text: &str) -> std::result::Result<Vec<String>, RelayListError> {
    let mut urls: Vec<String> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !line.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
            return Err(RelayListError::NotHttps(line.to_owned()));
        }
        RelaySetting::custom([line]).map_err(|_| RelayListError::Invalid(line.to_owned()))?;
        if !urls.iter().any(|u| u == line) {
            urls.push(line.to_owned());
        }
    }
    if urls.len() > MAX_RELAYS { Err(RelayListError::TooMany) } else { Ok(urls) }
}

/// The text of the `relays` file of `dir` (empty when there is none).
pub fn read_relays_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(RELAYS_FILE)).unwrap_or_default()
}

/// The validated list of the `relays` file of `dir`.
pub fn load_relays_from(dir: &Path) -> std::result::Result<Vec<String>, RelayListError> {
    parse_relay_list(&read_relays_text(dir))
}

/// Saves a validated list (an empty one removes the file: back to the public relays). Nothing is touched when the file already
/// holds this list.
pub fn save_relays_to(dir: &Path, urls: &[String]) {
    let path = dir.join(RELAYS_FILE);
    if load_relays_from(dir).is_ok_and(|current| current == urls) {
        return;
    }
    if urls.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(path, urls.join("\n") + "\n");
}

/// The configuration with the custom relays of `dir` when it asks for the public ones (an explicit choice, such as "no relay" in
/// tests, is kept). A file that is not valid is an error, never a silent fallback to the public relays.
pub fn with_relays_file(mut cfg: NetConfig, dir: Option<&Path>) -> std::result::Result<NetConfig, RelayListError> {
    if let (Some(dir), RelaySetting::Default) = (dir, &cfg.relays) {
        let urls = load_relays_from(dir)?;
        cfg.relays = RelaySetting::custom(&urls).map_err(|e| RelayListError::Invalid(e.0))?;
    }
    Ok(cfg)
}

/* ------------------------------------------------------------------------------------------------------ network detection */

/// Notices a change of the local network without any dependency: the address the system would use to reach the Internet is looked
/// at (a UDP "connect" sends nothing). It vanishes when the network is gone and differs when another interface takes over.
#[derive(Debug, Default)]
pub struct LocalAddrWatch {
    /// What the last look found: `None` before the first look, `Some(None)` when there was no route.
    last: Option<Option<IpAddr>>,
}

fn route_address() -> Option<IpAddr> {
    let probe = |bind: &str, to: &str| -> Option<IpAddr> {
        let socket = UdpSocket::bind(bind).ok()?;
        socket.connect(to).ok()?;
        socket.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
    };
    probe("0.0.0.0:0", "1.1.1.1:53").or_else(|| probe("[::]:0", "[2606:4700:4700::1111]:53"))
}

impl LocalAddrWatch {
    /// Feeds one look: `true` when the network is usable now and was not (or was another) at the previous look.
    pub fn observe(&mut self, now: Option<IpAddr>) -> bool {
        let fire = now.is_some() && self.last.is_some_and(|before| before != now);
        self.last = Some(now);
        fire
    }
}

impl NetWatch for LocalAddrWatch {
    fn changed(&mut self) -> bool {
        self.observe(route_address())
    }
}

/* ----------------------------------------------------------------------------------------------------------------- host */

/// What the host side keeps about the session it started (shared with the thread that starts it).
#[derive(Default)]
struct Live {
    transport: Weak<Mutex<IrohTransport>>,
    addr: Option<HostAddress>,
}

/// "Start sharing" over iroh. Public relays of n0 by default (decision D1), or the list of the `relays` file; `NetConfig::loopback()`
/// for tests.
pub struct IrohHostNet {
    cfg: NetConfig,
    /// Folder of the `relays` file, read at each start (`None`: the configuration is used as it is).
    relays_dir: Option<PathBuf>,
    live: Arc<Mutex<Live>>,
    watch: Option<Box<dyn NetWatch>>,
    checked: Option<Instant>,
}

impl IrohHostNet {
    pub fn new(cfg: NetConfig) -> Self {
        Self { cfg, relays_dir: None, live: Arc::default(), watch: None, checked: None }
    }

    /// Reads the custom relays from `dir` at each start.
    pub fn with_relays_dir(mut self, dir: PathBuf) -> Self {
        self.relays_dir = Some(dir);
        self
    }

    /// The host tells iroh when the local network changed (the real application plugs `LocalAddrWatch`).
    pub fn with_net_watch(mut self, watch: Box<dyn NetWatch>) -> Self {
        self.watch = Some(watch);
        self
    }

    /// The configuration of the next start: the relays of the file when it holds some.
    pub fn config(&self) -> std::result::Result<NetConfig, String> {
        with_relays_file(self.cfg.clone(), self.relays_dir.as_deref()).map_err(|e| e.message())
    }
}

/// Binds the endpoint and waits for the relay and the addresses (`ready_timeout`): runs on a thread, or blocks in tests.
fn open_host(cfg: NetConfig, live: &Mutex<Live>, cancel: &AtomicBool) -> StartResult {
    let transport = IrohTransport::host(cfg).map_err(|e| e.to_string())?;
    let host = transport.local_id().ok_or_else(|| "the network endpoint has no identity".to_owned())?;
    let relays = transport.hints().map_err(|e| e.to_string())?;
    if cancel.load(Ordering::SeqCst) {
        return Err(t("Cancelled")); // the transport is dropped here, on this thread
    }
    let addr = HostAddress { host, relays };
    let shared = Arc::new(Mutex::new(transport));
    *live.lock().unwrap_or_else(PoisonError::into_inner) = Live { transport: Arc::downgrade(&shared), addr: Some(addr.clone()) };
    Ok((Box::new(SharedHost(shared)), addr))
}

/// The transport handed to the session host. The `Weak` kept by [`IrohHostNet`] dies with it: stopping the session releases the
/// endpoint, the sockets and the network threads.
struct SharedHost(Arc<Mutex<IrohTransport>>);

impl HostTransport for SharedHost {
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        let conn = Transport::accept(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))?;
        Some(Box::new(conn))
    }

    /// Closing waits for the last frames (the final `Bye`) to be delivered: on its own thread, so that "Stop sharing" never freezes.
    fn release(self: Box<Self>) {
        close_in_background(move || drop(self));
    }
}

impl SessionNet for IrohHostNet {
    fn start(&mut self) -> std::result::Result<(Box<dyn HostTransport>, HostAddress), String> {
        let cfg = self.config()?;
        let (transport, addr) = open_host(cfg, &self.live, &AtomicBool::new(false))?;
        self.checked = Some(Instant::now());
        Ok((transport as Box<dyn HostTransport>, addr))
    }

    fn start_job(&mut self) -> Option<StartJob> {
        let cfg = self.config();
        let live = Arc::clone(&self.live);
        self.checked = Some(Instant::now());
        Some(Box::new(move |cancel| open_host(cfg?, &live, cancel)))
    }

    fn refresh(&mut self) -> Option<HostAddress> {
        if self.checked.is_some_and(|c| c.elapsed() < REFRESH_EVERY) {
            return None;
        }
        self.checked = Some(Instant::now());
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        let transport = live.transport.upgrade()?;
        if self.watch.as_mut().is_some_and(|w| w.changed()) {
            transport.lock().unwrap_or_else(PoisonError::into_inner).network_change();
        }
        let hints = transport.lock().unwrap_or_else(PoisonError::into_inner).current_hints();
        let current = live.addr.as_mut()?;
        if hints.iter().all(|h| current.relays.contains(h)) {
            return None;
        }
        // what was already given out stays valid: new hints go first, the old ones follow
        let mut merged = hints;
        let old: Vec<String> = current.relays.iter().filter(|h| !merged.contains(h)).cloned().collect();
        merged.extend(old);
        merged.truncate(MAX_HINTS);
        current.relays = merged;
        Some(current.clone())
    }
}

/// The host's network for the real application (public relays, or the `relays` file of the data folder). Holds a configuration
/// only: nothing connects before `start`.
pub fn host_net() -> Box<dyn SessionNet> {
    Box::new(
        IrohHostNet::new(NetConfig { ready_timeout: HOST_READY, ..NetConfig::default() })
            .with_relays_dir(inkboard_mcp::live::data_dir())
            .with_net_watch(Box::new(LocalAddrWatch::default())),
    )
}

/* ---------------------------------------------------------------------------------------------------------------- guest */

/// The link of a guest: dials on a thread and returns at once a connection that opens when the handshake is done.
struct NetLink {
    cfg: NetConfig,
    ticket: Ticket,
    /// The guest's endpoint, created by the first dial and kept for the whole session: the host recognizes a reconnecting guest by
    /// this identity. Dropped with the link, on a thread of its own (a dial still running keeps it alive until it is cancelled).
    transport: Option<Arc<Mutex<Option<IrohTransport>>>>,
    failure: Arc<Mutex<Option<ConnectError>>>,
    /// Set when the network changed while no dial could use it at once: the next dial tells the endpoint first.
    poke: Arc<AtomicBool>,
}

fn dial(
    slot: &Mutex<Option<IrohTransport>>,
    cfg: &NetConfig,
    ticket: &Ticket,
    cancel: &CancelToken,
    poke: &AtomicBool,
) -> std::result::Result<IrohConn, ConnectError> {
    let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if cancel.is_cancelled() {
        return Err(ConnectError::new(ConnectErrorKind::Other, "the attempt was cancelled"));
    }
    if guard.is_none() {
        *guard = Some(IrohTransport::guest(cfg.clone()).map_err(|e| connect_error(&e))?);
    }
    let transport = guard.as_mut().expect("created just above");
    if poke.swap(false, Ordering::SeqCst) {
        transport.network_change();
    }
    transport.connect_ticket_cancellable(ticket, cancel).map_err(|e| connect_error(&e))
}

impl GuestLink for NetLink {
    /// The ticket carries everything (identity, relays, addresses): `host` is only its identity.
    fn connect(&mut self, _host: &str) -> std::result::Result<Box<dyn Connection>, ConnectError> {
        let (tx, rx) = channel();
        let slot = Arc::clone(self.transport.as_ref().ok_or_else(|| ConnectError::new(ConnectErrorKind::Other, "the link is closed"))?);
        let (cfg, ticket, poke, cancel) = (self.cfg.clone(), self.ticket.clone(), Arc::clone(&self.poke), CancelToken::new());
        let dial_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("inkboard-join".to_owned())
            .spawn(move || {
                let _ = tx.send(dial(&slot, &cfg, &ticket, &dial_cancel, &poke));
            })
            .map_err(|e| ConnectError::new(ConnectErrorKind::Other, &format!("cannot start the connection thread: {e}")))?;
        Ok(Box::new(Dialing::new(self.ticket.host.clone(), rx, Arc::clone(&self.failure), cancel)))
    }

    fn take_failure(&mut self) -> Option<ConnectError> {
        self.failure.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// iroh looks at the interfaces again and probes its paths. When a dial holds the endpoint, the dial that follows does it.
    fn network_changed(&mut self) {
        self.poke.store(true, Ordering::SeqCst);
        if let Some(slot) = &self.transport
            && let Ok(guard) = slot.try_lock()
            && let Some(transport) = guard.as_ref()
        {
            transport.network_change();
            self.poke.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for NetLink {
    /// Closing the endpoint waits for the last frames (the `Bye` of a guest that leaves): not on the interface's thread.
    fn drop(&mut self) {
        if let Some(slot) = self.transport.take() {
            close_in_background(move || drop(slot));
        }
    }
}

/// Looks at the relay with `inkboard_net::diagnose_relay`, so that "blocked by a security gateway (Zscaler)", "does not answer" and
/// "answers normally" can be told apart.
pub struct NetDiagnoser {
    pub timeout: Duration,
    /// Relays configured by the user (host names or URLs): besides the relays of n0, the only ones that may be on a private address.
    pub trusted: Vec<String>,
    /// Test mode: diagnose loopback / plain-HTTP addresses too.
    pub allow_private: bool,
    /// Folder of the `relays` file: its relays are configured, hence known (read at each diagnosis, the file may have just changed).
    pub relays_dir: Option<PathBuf>,
}

impl RelayDiagnoser for NetDiagnoser {
    fn diagnose(&self, relay_url: &str) -> Diagnosis {
        let mut trusted = self.trusted.clone();
        trusted.extend(self.relays_dir.as_deref().and_then(|d| load_relays_from(d).ok()).unwrap_or_default());
        let d = diagnose_relay_with(relay_url, self.timeout, &DiagnoseOptions { trusted, allow_private: self.allow_private });
        Diagnosis {
            reachable: d.reachable_tcp,
            http_status: d.http_status,
            server_header: d.server_header,
            // other gateways are still recognized by their status code (401, 403, 407, 451, 511) in `classify_failure`
            gateway: (d.gateway == Some(GatewayKind::Zscaler)).then(|| "Zscaler".to_owned()),
            summary: d.summary,
            ..Diagnosis::default()
        }
    }
}

/// What a guest session needs from the network.
pub struct GuestNet {
    pub net: NetConfig,
    pub session: GuestConfig,
    pub diagnoser: Arc<dyn RelayDiagnoser>,
    /// Folder of the `relays` file, read each time a session is built (`None`: `net` is used as it is).
    pub relays_dir: Option<PathBuf>,
    /// The session looks at the computer's network (`LocalAddrWatch`) and retries at once when it comes back.
    pub watch_network: bool,
}

impl GuestNet {
    /// The real application: public relays (or the `relays` file), 15 s handshake, relay diagnosis limited to 5 s.
    pub fn public() -> Self {
        Self {
            net: NetConfig { connect_timeout: GUEST_CONNECT, ..NetConfig::default() },
            session: GuestConfig::default(),
            diagnoser: Arc::new(NetDiagnoser {
                timeout: DIAGNOSE,
                trusted: Vec::new(),
                allow_private: false,
                relays_dir: Some(inkboard_mcp::live::data_dir()),
            }),
            relays_dir: Some(inkboard_mcp::live::data_dir()),
            watch_network: true,
        }
    }

    /// The network configuration of a session being built: the relays of the file when it holds some (the window validated it
    /// just before; a file that became invalid in between leaves the configuration as it is).
    pub fn config(&self) -> NetConfig {
        with_relays_file(self.net.clone(), self.relays_dir.as_deref()).unwrap_or_else(|_| self.net.clone())
    }

    /// What `app.join.factory` holds. Building it connects to nothing: the endpoint is created when the session first dials.
    pub fn into_factory(self) -> SessionFactory {
        Box::new(move |ticket, name| {
            let link = Box::new(NetLink {
                cfg: self.config(),
                ticket: ticket.clone(),
                transport: Some(Arc::new(Mutex::new(None))),
                failure: Arc::new(Mutex::new(None)),
                poke: Arc::new(AtomicBool::new(false)),
            });
            // the relays of the `relays` file are configured by the user: known, no confirmation, advice allowed
            let mut config = self.session.clone();
            if let RelaySetting::Custom(urls) = &link.cfg.relays {
                config.trusted_relays.extend(urls.iter().map(ToString::to_string));
            }
            let mut session = GuestSession::new(link, Arc::clone(&self.diagnoser), ticket, name, Box::new(OsEntropy), Clock::real(), config);
            if self.watch_network {
                session.set_net_watch(Box::new(LocalAddrWatch::default()));
            }
            session
        })
    }
}

/// The guest factory of the real application.
pub fn guest_factory() -> SessionFactory {
    GuestNet::public().into_factory()
}
