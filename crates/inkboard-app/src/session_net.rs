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
//! - Failures: the transport's `Error::Network` becomes a [`ConnectError`] for the classification of `session_guest`; the relay is
//!   diagnosed on a thread with `inkboard_net::diagnose_relay`.

use crate::join_ui::SessionFactory;
use crate::session_guest::{Clock, ConnectError, ConnectErrorKind, Diagnosis, GuestConfig, GuestLink, GuestSession, RelayDiagnoser};
use crate::session_host::{HostAddress, HostTransport, SessionNet};
use inkboard_net::diagnose::{DiagnoseOptions, diagnose_relay_with};
use inkboard_net::ticket::MAX_HINTS;
use inkboard_net::{GatewayKind, IrohConn, IrohTransport, NetConfig};
use inkboard_session::crypto::{OsEntropy, Ticket};
use inkboard_session::error::{Error, NetCause, Result};
use inkboard_session::transport::{Connection, Transport};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

/// How long "Start sharing" waits for the relay and the local addresses (the interface is busy meanwhile). What arrives later is
/// added to the links by `refresh`.
const HOST_READY: Duration = Duration::from_secs(3);
/// A guest gives up on a handshake after this long (shorter than the session's own 20 s, so that the network says why).
const GUEST_CONNECT: Duration = Duration::from_secs(15);
/// How long the diagnosis of a relay may take.
const DIAGNOSE: Duration = Duration::from_secs(5);
/// The host's address is looked at again this often.
const REFRESH_EVERY: Duration = Duration::from_secs(1);
/// Frames a connection that is still being established holds back (the `Hello` and what follows).
const MAX_QUEUED: usize = 1024;

/* ----------------------------------------------------------------------------------------------------------------- host */

/// "Start sharing" over iroh. Public relays of n0 by default (decision D1); `NetConfig::loopback()` for tests.
pub struct IrohHostNet {
    cfg: NetConfig,
    live: Weak<Mutex<IrohTransport>>,
    addr: Option<HostAddress>,
    checked: Option<Instant>,
}

impl IrohHostNet {
    pub fn new(cfg: NetConfig) -> Self {
        Self { cfg, live: Weak::new(), addr: None, checked: None }
    }
}

/// The transport handed to the session host. The `Weak` kept by [`IrohHostNet`] dies with it: stopping the session releases the
/// endpoint, the sockets and the network threads.
struct SharedHost(Arc<Mutex<IrohTransport>>);

impl HostTransport for SharedHost {
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        let conn = Transport::accept(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))?;
        Some(Box::new(conn))
    }
}

impl SessionNet for IrohHostNet {
    fn start(&mut self) -> std::result::Result<(Box<dyn HostTransport>, HostAddress), String> {
        let transport = IrohTransport::host(self.cfg.clone()).map_err(|e| e.to_string())?;
        let host = transport.local_id().ok_or_else(|| "the network endpoint has no identity".to_owned())?;
        let relays = transport.hints().map_err(|e| e.to_string())?;
        let addr = HostAddress { host, relays };
        let shared = Arc::new(Mutex::new(transport));
        self.live = Arc::downgrade(&shared);
        self.addr = Some(addr.clone());
        self.checked = Some(Instant::now());
        Ok((Box::new(SharedHost(shared)), addr))
    }

    fn refresh(&mut self) -> Option<HostAddress> {
        if self.checked.is_some_and(|c| c.elapsed() < REFRESH_EVERY) {
            return None;
        }
        self.checked = Some(Instant::now());
        let hints = self.live.upgrade()?.lock().unwrap_or_else(PoisonError::into_inner).current_hints();
        let current = self.addr.as_mut()?;
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

/// The host's network for the real application (public relays). Holds a configuration only: nothing connects before `start`.
pub fn host_net() -> Box<dyn SessionNet> {
    Box::new(IrohHostNet::new(NetConfig { ready_timeout: HOST_READY, ..NetConfig::default() }))
}

/* ---------------------------------------------------------------------------------------------------------------- guest */

fn connect_error(e: &Error) -> ConnectError {
    let kind = match e {
        Error::Network { cause: NetCause::Local, .. } => ConnectErrorKind::NoNetwork,
        Error::Network { cause: NetCause::TimedOut, .. } => ConnectErrorKind::TimedOut,
        _ => ConnectErrorKind::Other,
    };
    ConnectError::new(kind, &e.to_string())
}

/// The link of a guest: dials on a thread and returns at once a connection that opens when the handshake is done.
struct NetLink {
    cfg: NetConfig,
    ticket: Ticket,
    /// The guest's endpoint, created by the first dial and kept for the whole session: the host recognizes a reconnecting guest by
    /// this identity. Dropped with the link (a dial still running keeps it alive until it ends, at most `connect_timeout`).
    transport: Arc<Mutex<Option<IrohTransport>>>,
    failure: Arc<Mutex<Option<ConnectError>>>,
}

fn dial(slot: &Mutex<Option<IrohTransport>>, cfg: &NetConfig, ticket: &Ticket) -> std::result::Result<IrohConn, ConnectError> {
    let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        *guard = Some(IrohTransport::guest(cfg.clone()).map_err(|e| connect_error(&e))?);
    }
    guard.as_mut().expect("created just above").connect_ticket(ticket).map_err(|e| connect_error(&e))
}

impl GuestLink for NetLink {
    /// The ticket carries everything (identity, relays, addresses): `host` is only its identity.
    fn connect(&mut self, _host: &str) -> std::result::Result<Box<dyn Connection>, ConnectError> {
        let (tx, rx) = channel();
        let (slot, cfg, ticket) = (Arc::clone(&self.transport), self.cfg.clone(), self.ticket.clone());
        std::thread::Builder::new()
            .name("inkboard-join".to_owned())
            .spawn(move || {
                let _ = tx.send(dial(&slot, &cfg, &ticket));
            })
            .map_err(|e| ConnectError::new(ConnectErrorKind::Other, &format!("cannot start the connection thread: {e}")))?;
        Ok(Box::new(Dialing { peer: self.ticket.host.clone(), rx: Some(rx), conn: None, queued: Vec::new(), dead: false, failure: Arc::clone(&self.failure) }))
    }

    fn take_failure(&mut self) -> Option<ConnectError> {
        self.failure.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// A connection being established: frames sent meanwhile are kept and go out once it is open; if it never opens, the reason is left
/// for [`GuestLink::take_failure`].
struct Dialing {
    peer: String,
    rx: Option<Receiver<std::result::Result<IrohConn, ConnectError>>>,
    conn: Option<IrohConn>,
    queued: Vec<Vec<u8>>,
    dead: bool,
    failure: Arc<Mutex<Option<ConnectError>>>,
}

impl Dialing {
    fn settle(&mut self) {
        let Some(rx) = self.rx.as_ref() else { return };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(Ok(mut conn)) => {
                for frame in self.queued.drain(..) {
                    let _ = conn.send(&frame);
                }
                self.conn = Some(conn);
                self.rx = None;
            }
            Ok(Err(e)) => self.fail(e),
            Err(TryRecvError::Disconnected) => self.fail(ConnectError::new(ConnectErrorKind::Other, "the connection thread ended without an answer")),
        }
    }

    fn fail(&mut self, e: ConnectError) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
        self.dead = true;
        self.rx = None;
        self.queued.clear();
    }
}

impl Connection for Dialing {
    fn peer(&self) -> &str {
        &self.peer
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        self.settle();
        match self.conn.as_mut() {
            Some(c) => c.send(frame),
            None if self.dead => Err(Error::Closed),
            None if self.queued.len() >= MAX_QUEUED => {
                Err(Error::Network { cause: NetCause::Congested, detail: "too many frames waiting for the connection".to_owned() })
            }
            None => {
                self.queued.push(frame.to_vec());
                Ok(())
            }
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.settle();
        self.conn.as_mut()?.recv()
    }

    fn is_open(&self) -> bool {
        self.conn.as_ref().is_some_and(Connection::is_open)
    }

    fn close(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.dead = true;
        self.rx = None;
        self.queued.clear();
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
}

impl RelayDiagnoser for NetDiagnoser {
    fn diagnose(&self, relay_url: &str) -> Diagnosis {
        let d = diagnose_relay_with(relay_url, self.timeout, &DiagnoseOptions { trusted: self.trusted.clone(), allow_private: self.allow_private });
        Diagnosis {
            reachable: d.reachable_tcp,
            http_status: d.http_status,
            server_header: d.server_header,
            // other gateways are still recognized by their status code (401, 403, 407, 451, 511) in `classify_failure`
            gateway: (d.gateway == Some(GatewayKind::Zscaler)).then(|| "Zscaler".to_owned()),
            summary: d.summary,
        }
    }
}

/// What a guest session needs from the network.
pub struct GuestNet {
    pub net: NetConfig,
    pub session: GuestConfig,
    pub diagnoser: Arc<dyn RelayDiagnoser>,
}

impl GuestNet {
    /// The real application: public relays, 15 s handshake, relay diagnosis limited to 5 s.
    pub fn public() -> Self {
        Self {
            net: NetConfig { connect_timeout: GUEST_CONNECT, ..NetConfig::default() },
            session: GuestConfig::default(),
            diagnoser: Arc::new(NetDiagnoser { timeout: DIAGNOSE, trusted: Vec::new(), allow_private: false }),
        }
    }

    /// What `app.join.factory` holds. Building it connects to nothing: the endpoint is created when the session first dials.
    pub fn into_factory(self) -> SessionFactory {
        Box::new(move |ticket, name| {
            let link =
                Box::new(NetLink { cfg: self.net.clone(), ticket: ticket.clone(), transport: Arc::new(Mutex::new(None)), failure: Arc::new(Mutex::new(None)) });
            GuestSession::new(link, Arc::clone(&self.diagnoser), ticket, name, Box::new(OsEntropy), Clock::real(), self.session.clone())
        })
    }
}

/// The guest factory of the real application.
pub fn guest_factory() -> SessionFactory {
    GuestNet::public().into_factory()
}
