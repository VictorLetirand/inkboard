//! `inkboard_session::transport::{Transport, Connection}` over iroh (QUIC).
//!
//! One QUIC connection per guest, one bidirectional stream per connection. The guest opens the stream and writes a 4-byte preamble
//! (`IBN1`) so that the host sees it at once (QUIC streams are invisible to the peer until data flows). After that, each frame is
//! `u32 length (little endian) | bytes`; the bytes are the frames of `inkboard-session` (already encrypted, opaque here).
//!
//! The session traits are polling and synchronous; this module hides a small tokio runtime owned by [`IrohTransport`] and one
//! reader task and one writer task per connection. `connect`/`host`/`ticket` block the calling thread (never call them from
//! inside an async runtime); `send`/`recv`/`accept` never block.

use crate::config::{NetConfig, RelaySetting};
use crate::ticket::{MAX_HINTS, addr_to_hints, ticket_to_addr};
use inkboard_session::crypto::{HostKeys, Ticket};
use inkboard_session::error::{Error, NetCause, Result};
use inkboard_session::limits::Violation;
use inkboard_session::proto::Role;
use inkboard_session::transport::{Connection, Transport};
use iroh::endpoint::{Connection as QuicConn, ConnectionError, Incoming, ReadError, ReadExactError, RecvStream, SendStream, VarInt, presets};
use iroh::endpoint::{IdleTimeout, QuicTransportConfig};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, TransportAddr};
use std::fmt::Display;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::runtime::{Builder as RuntimeBuilder, Handle, Runtime};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, timeout};

/// ALPN of the session protocol (the trailing number is the transport framing version).
pub const ALPN: &[u8] = b"inkboard/session/1";
const PREAMBLE: &[u8; 4] = b"IBN1";
/// Application close codes.
const CLOSE_NORMAL: u32 = 0;
const CLOSE_REFUSED: u32 = 1;
/// Frames buffered per direction before back-pressure (reader) or `Congested` (sender).
const QUEUE: usize = 1024;
/// Connections accepted by the transport but not yet taken with `accept`.
const PENDING_ACCEPT: usize = 64;
/// A peer that connects but does not send the preamble in time is dropped.
const PREAMBLE_TIMEOUT: Duration = Duration::from_secs(10);
/// On a local `close`, how long to wait for the peer to confirm it has read everything.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// Dropping a transport waits this long (at most) for the connections closed just before to be flushed.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

fn net(cause: NetCause, detail: impl Into<String>) -> Error {
    Error::Network { cause, detail: detail.into() }
}

fn local(e: impl Display) -> Error {
    net(NetCause::Local, e.to_string())
}

/* ------------------------------------------------------------- Connection */

/// State shared by a connection handle and its two tasks.
struct Shared {
    open: AtomicBool,
    /// Why the connection ended, when it is not a plain close.
    fault: Mutex<Option<Error>>,
    /// Set when the peer is gone (end of stream or error): tells the writer to stop.
    gone: watch::Sender<bool>,
    /// A local close is counted in the transport's `flushing` counter until the writer ends (see `flush_started`).
    flush_counted: AtomicBool,
    writer_done: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            open: AtomicBool::new(true),
            fault: Mutex::new(None),
            gone: watch::channel(false).0,
            flush_counted: AtomicBool::new(false),
            writer_done: AtomicBool::new(false),
        })
    }

    /// The handle stopped writing: until the writer has drained its queue, dropping the transport must wait (frames such as the
    /// final `Bye` would be lost otherwise). Exactly one of this call and `writer_ended` undoes the count.
    fn flush_started(&self, flushing: &AtomicUsize) {
        flushing.fetch_add(1, Ordering::SeqCst);
        self.flush_counted.store(true, Ordering::SeqCst);
        if self.writer_done.load(Ordering::SeqCst) {
            self.flush_finished(flushing);
        }
    }

    fn flush_finished(&self, flushing: &AtomicUsize) {
        if self.flush_counted.swap(false, Ordering::SeqCst) {
            flushing.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn writer_ended(&self, flushing: &AtomicUsize) {
        self.writer_done.store(true, Ordering::SeqCst);
        self.flush_finished(flushing);
    }

    /// The first call wins and records the cause.
    fn mark_closed(&self, fault: Option<Error>) {
        if self.open.swap(false, Ordering::SeqCst) {
            *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = fault;
        }
    }

    fn peer_gone(&self, fault: Option<Error>) {
        self.mark_closed(fault);
        self.gone.send_replace(true);
    }
}

/// One connection to one peer. Frames received before the end of the connection stay readable with `recv`.
pub struct IrohConn {
    peer: String,
    out: Option<mpsc::Sender<Vec<u8>>>,
    inbox: mpsc::Receiver<Vec<u8>>,
    shared: Arc<Shared>,
    conn: QuicConn,
    max_frame: usize,
    /// Connections of the transport that are closing but not yet flushed.
    flushing: Arc<AtomicUsize>,
}

impl std::fmt::Debug for IrohConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohConn").field("peer", &self.peer).field("open", &self.is_open()).finish_non_exhaustive()
    }
}

impl IrohConn {
    /// Starts the two tasks. Must run inside the transport's runtime.
    fn start(conn: QuicConn, send: SendStream, recv: RecvStream, max_frame: usize, flushing: &Arc<AtomicUsize>) -> Self {
        let shared = Shared::new();
        let (out_tx, out_rx) = mpsc::channel(QUEUE);
        let (in_tx, in_rx) = mpsc::channel(QUEUE);
        tokio::spawn(read_loop(recv, in_tx, shared.clone(), conn.clone(), max_frame));
        tokio::spawn(write_loop(send, out_rx, shared.clone(), conn.clone(), flushing.clone()));
        Self { peer: conn.remote_id().to_string(), out: Some(out_tx), inbox: in_rx, shared, conn, max_frame, flushing: flushing.clone() }
    }

    /// Drops the queue's sender: the writer drains what is queued, then closes. Counted so that the transport can wait for it.
    fn stop_writing(&mut self) {
        if self.out.is_some() {
            self.shared.flush_started(&self.flushing);
            self.out = None;
        }
    }

    /// Why the connection is closed: `None` while open; `Error::Closed` for a plain close, otherwise the cause
    /// (`Network { TimedOut | Refused | .. }`).
    pub fn close_cause(&self) -> Option<Error> {
        if self.shared.open.load(Ordering::SeqCst) {
            return None;
        }
        Some(self.shared.fault.lock().unwrap_or_else(PoisonError::into_inner).clone().unwrap_or(Error::Closed))
    }

    /// Host side: closes the connection and tells the peer that it was refused (it sees `Network { Refused }`).
    pub fn refuse(&mut self) {
        self.shared.mark_closed(None);
        self.stop_writing();
        self.conn.close(VarInt::from_u32(CLOSE_REFUSED), b"refused");
    }
}

impl Connection for IrohConn {
    fn peer(&self) -> &str {
        &self.peer
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        if frame.len() > self.max_frame {
            return Err(Violation::MessageTooLarge { size: frame.len(), max: self.max_frame }.into());
        }
        let Some(out) = self.out.as_ref().filter(|_| self.is_open()) else { return Err(self.close_cause().unwrap_or(Error::Closed)) };
        match out.try_send(frame.to_vec()) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(net(NetCause::Congested, "the outgoing queue is full")),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(self.close_cause().unwrap_or(Error::Closed)),
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.inbox.try_recv().ok()
    }

    fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::SeqCst)
    }

    /// Flushes the queued frames in the background, then closes the connection.
    fn close(&mut self) {
        self.shared.mark_closed(None);
        self.stop_writing();
    }
}

impl Drop for IrohConn {
    fn drop(&mut self) {
        self.close();
    }
}

fn map_conn_error(e: &ConnectionError) -> Error {
    match e {
        ConnectionError::TimedOut => net(NetCause::TimedOut, "nothing received from the peer within the idle timeout"),
        ConnectionError::ApplicationClosed(c) if c.error_code == VarInt::from_u32(CLOSE_REFUSED) => net(NetCause::Refused, "the host refused the connection"),
        _ => Error::Closed,
    }
}

fn read_fault(e: &ReadError, conn: &QuicConn) -> Error {
    match e {
        ReadError::ConnectionLost(ce) => map_conn_error(ce),
        _ => conn.close_reason().map_or(Error::Closed, |ce| map_conn_error(&ce)),
    }
}

enum FrameError {
    Read(ReadError),
    Truncated,
    TooLarge(usize),
}

/// `Ok(None)` at a clean end of stream (between two frames).
async fn read_frame(recv: &mut RecvStream, max_frame: usize) -> std::result::Result<Option<Vec<u8>>, FrameError> {
    let mut len = [0u8; 4];
    match recv.read_exact(&mut len).await {
        Ok(()) => {}
        Err(ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(ReadExactError::FinishedEarly(_)) => return Err(FrameError::Truncated),
        Err(ReadExactError::ReadError(e)) => return Err(FrameError::Read(e)),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > max_frame {
        return Err(FrameError::TooLarge(n));
    }
    let mut frame = vec![0u8; n];
    match recv.read_exact(&mut frame).await {
        Ok(()) => Ok(Some(frame)),
        Err(ReadExactError::FinishedEarly(_)) => Err(FrameError::Truncated),
        Err(ReadExactError::ReadError(e)) => Err(FrameError::Read(e)),
    }
}

async fn read_loop(mut recv: RecvStream, inbox: mpsc::Sender<Vec<u8>>, shared: Arc<Shared>, conn: QuicConn, max_frame: usize) {
    loop {
        match read_frame(&mut recv, max_frame).await {
            // `send` fails only when the handle is gone; the writer then flushes and closes.
            Ok(Some(frame)) => {
                if inbox.send(frame).await.is_err() {
                    break;
                }
            }
            Ok(None) => return shared.peer_gone(None),
            Err(FrameError::Read(e)) => return shared.peer_gone(Some(read_fault(&e, &conn))),
            Err(FrameError::Truncated) => return shared.peer_gone(None),
            Err(FrameError::TooLarge(size)) => {
                shared.peer_gone(Some(Violation::MessageTooLarge { size, max: max_frame }.into()));
                conn.close(VarInt::from_u32(CLOSE_REFUSED), b"frame too large");
                return;
            }
        }
    }
}

async fn write_loop(mut send: SendStream, mut queue: mpsc::Receiver<Vec<u8>>, shared: Arc<Shared>, conn: QuicConn, flushing: Arc<AtomicUsize>) {
    let mut gone = shared.gone.subscribe();
    // `true` when the local side asked to close: then the queue is drained first and the peer given time to read it.
    let local_close = loop {
        tokio::select! {
            frame = queue.recv() => match frame {
                None => break true,
                Some(frame) => {
                    let len = (frame.len() as u32).to_le_bytes();
                    let written = async {
                        send.write_all(&len).await?;
                        send.write_all(&frame).await
                    };
                    if let Err(e) = written.await {
                        let fault = match e {
                            iroh::endpoint::WriteError::ConnectionLost(ce) => map_conn_error(&ce),
                            _ => conn.close_reason().map_or(Error::Closed, |ce| map_conn_error(&ce)),
                        };
                        shared.peer_gone(Some(fault));
                        break false;
                    }
                }
            },
            // The wrapper drops the watch guard at once (it is not `Send`).
            () = async { gone.wait_for(|g| *g).await.ok(); } => break false,
        }
    };
    send.finish().ok();
    if local_close {
        // The side that sends last must not close before the peer has read everything: wait for the peer to close.
        timeout(FLUSH_TIMEOUT, conn.closed()).await.ok();
    }
    conn.close(VarInt::from_u32(CLOSE_NORMAL), b"bye");
    shared.writer_ended(&flushing);
}

/* -------------------------------------------------------------- Endpoints */

fn quic_config(idle: Duration) -> QuicTransportConfig {
    let idle: IdleTimeout = idle.clamp(Duration::from_secs(1), Duration::from_secs(3600)).try_into().expect("clamped idle timeout fits");
    QuicTransportConfig::builder().max_idle_timeout(Some(idle)).build()
}

async fn bind_host(cfg: &NetConfig) -> Result<Endpoint> {
    let mut builder =
        Endpoint::builder(presets::Minimal).alpns(vec![ALPN.to_vec()]).relay_mode(cfg.relays.mode()).transport_config(quic_config(cfg.idle_timeout));
    if cfg.loopback_only {
        builder = builder.clear_ip_transports().bind_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).map_err(local)?;
    }
    builder.bind().await.map_err(local)
}

/// Relays a guest uses to reach the host of a given ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GuestRelays {
    Off,
    Default,
    List(Vec<RelayUrl>),
}

impl GuestRelays {
    fn for_ticket(setting: &RelaySetting, ticket_relays: &[RelayUrl]) -> Self {
        let mut list: Vec<RelayUrl> = match setting {
            RelaySetting::Disabled => return Self::Off,
            RelaySetting::Default => Vec::new(),
            RelaySetting::Custom(urls) => urls.clone(),
        };
        list.extend(ticket_relays.iter().cloned());
        list.sort();
        list.dedup();
        if list.is_empty() { Self::Default } else { Self::List(list) }
    }

    fn mode(&self) -> RelayMode {
        match self {
            Self::Off => RelayMode::Disabled,
            Self::Default => RelayMode::Default,
            Self::List(urls) => RelayMode::custom(urls.iter().cloned()),
        }
    }
}

async fn bind_guest(cfg: &NetConfig, relays: &GuestRelays) -> Result<Endpoint> {
    Endpoint::builder(presets::Minimal).relay_mode(relays.mode()).transport_config(quic_config(cfg.idle_timeout)).bind().await.map_err(local)
}

/// A failed dial: when relays were expected but the endpoint never reached one, the relay is the culprit.
fn dial_failure(endpoint: &Endpoint, relays_on: bool, detail: String) -> Error {
    if relays_on && endpoint.addr().relay_urls().next().is_none() {
        net(NetCause::RelayUnreachable, format!("no connection to a relay ({detail})"))
    } else {
        net(NetCause::HostUnreachable, detail)
    }
}

/// Lets another thread abort a dial in progress (`connect_ticket_cancellable`): the attempt ends at once instead of at
/// `connect_timeout`. Cloning shares the same token.
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<watch::Sender<bool>>);

impl Default for CancelToken {
    fn default() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        // The wrapper drops the watch guard at once (it is not `Send`).
        async {
            rx.wait_for(|c| *c).await.ok();
        }
        .await;
    }
}

async fn dial(
    endpoint: &Endpoint,
    addr: EndpointAddr,
    cfg: &NetConfig,
    relays_on: bool,
    flushing: &Arc<AtomicUsize>,
    cancel: Option<&CancelToken>,
) -> Result<IrohConn> {
    let attempt = async {
        let conn = endpoint.connect(addr, ALPN).await.map_err(|e| dial_failure(endpoint, relays_on, e.to_string()))?;
        let (mut send, recv) = conn.open_bi().await.map_err(|e| map_conn_error(&e))?;
        send.write_all(PREAMBLE).await.map_err(|e| net(NetCause::HostUnreachable, e.to_string()))?;
        Ok::<_, Error>((conn, send, recv))
    };
    let aborted = async {
        match cancel {
            Some(c) => c.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let result = tokio::select! {
        r = timeout(cfg.connect_timeout, attempt) => r,
        () = aborted => return Err(Error::Closed),
    };
    match result {
        Ok(Ok((conn, send, recv))) => Ok(IrohConn::start(conn, send, recv, cfg.max_frame, flushing)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(dial_failure(endpoint, relays_on, format!("no answer within {} s", cfg.connect_timeout.as_secs_f32()))),
    }
}

/// Reads the preamble of a new connection and publishes it, or closes it as refused.
async fn welcome(incoming: Incoming, max_frame: usize, ready: mpsc::Sender<IrohConn>, flushing: Arc<AtomicUsize>) {
    let Ok(accepting) = incoming.accept() else { return };
    let Ok(conn) = accepting.await else { return };
    let opened = timeout(PREAMBLE_TIMEOUT, async {
        let (send, mut recv) = conn.accept_bi().await.ok()?;
        let mut preamble = [0u8; 4];
        recv.read_exact(&mut preamble).await.ok()?;
        (&preamble == PREAMBLE).then_some((send, recv))
    })
    .await;
    let Ok(Some((send, recv))) = opened else {
        conn.close(VarInt::from_u32(CLOSE_REFUSED), b"bad preamble");
        return;
    };
    let guest = IrohConn::start(conn, send, recv, max_frame, &flushing);
    if let Err(mpsc::error::TrySendError::Full(mut guest)) = ready.try_send(guest) {
        guest.refuse();
    }
}

async fn accept_loop(endpoint: Endpoint, max_frame: usize, ready: mpsc::Sender<IrohConn>, flushing: Arc<AtomicUsize>) {
    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(welcome(incoming, max_frame, ready.clone(), flushing.clone()));
    }
}

/// Addresses to put in a ticket: relays once the home relay is known, direct addresses once discovered (bounded by
/// `ready_timeout`; whatever is known then is used, so that a LAN without Internet still gets a usable link).
async fn collect_hints(endpoint: &Endpoint, cfg: &NetConfig) -> Vec<String> {
    let relays_on = !cfg.relays.is_disabled();
    let ready = async {
        loop {
            let addr = endpoint.addr();
            let relay_ok = !relays_on || addr.relay_urls().next().is_some();
            if relay_ok && (cfg.loopback_only || addr.ip_addrs().next().is_some()) {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    };
    timeout(cfg.ready_timeout, ready).await.ok();
    snapshot_hints(endpoint, cfg)
}

/// What is known now, without waiting.
fn snapshot_hints(endpoint: &Endpoint, cfg: &NetConfig) -> Vec<String> {
    let mut addr = endpoint.addr();
    addr.addrs.retain(|a| matches!(a, TransportAddr::Relay(_)) || matches!(a, TransportAddr::Ip(sa) if !sa.ip().is_loopback()));
    if cfg.loopback_only {
        addr.addrs.retain(TransportAddr::is_relay);
        addr.addrs.extend(endpoint.bound_sockets().into_iter().filter(|sa| sa.ip().is_loopback()).map(TransportAddr::Ip));
    }
    let mut hints = addr_to_hints(&addr);
    hints.truncate(MAX_HINTS);
    hints
}

/* -------------------------------------------------------------- Transport */

struct HostState {
    endpoint: Endpoint,
    ready: mpsc::Receiver<IrohConn>,
}

/// Native transport. Create it with [`IrohTransport::host`] (accepts guests) or [`IrohTransport::guest`] (connects to a host).
/// Keep it alive as long as its connections are in use: dropping it stops their tasks.
pub struct IrohTransport {
    // `Option` so that `Drop` can take the runtime out.
    rt: Option<Runtime>,
    cfg: NetConfig,
    host: Option<HostState>,
    guest: Option<(GuestRelays, Endpoint)>,
    /// Connections closed by their handle whose writer has not finished flushing: `drop` waits for them.
    flushing: Arc<AtomicUsize>,
}

impl std::fmt::Debug for IrohTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohTransport").field("host", &self.host.is_some()).finish_non_exhaustive()
    }
}

fn runtime() -> Result<Runtime> {
    RuntimeBuilder::new_multi_thread().worker_threads(2).thread_name("inkboard-net").enable_all().build().map_err(local)
}

impl IrohTransport {
    fn rt(&self) -> &Runtime {
        self.rt.as_ref().expect("the runtime lives as long as the transport")
    }

    /// Host side: binds an endpoint and starts accepting guests. Blocks briefly (socket binding only; relay and address
    /// discovery continue in the background, see [`IrohTransport::ticket`]).
    pub fn host(cfg: NetConfig) -> Result<Self> {
        let rt = runtime()?;
        let endpoint = rt.block_on(bind_host(&cfg))?;
        let (tx, ready) = mpsc::channel(PENDING_ACCEPT);
        let flushing = Arc::new(AtomicUsize::new(0));
        rt.spawn(accept_loop(endpoint.clone(), cfg.max_frame, tx, flushing.clone()));
        Ok(Self { rt: Some(rt), cfg, host: Some(HostState { endpoint, ready }), guest: None, flushing })
    }

    /// Guest side: nothing is bound until `connect_ticket`.
    pub fn guest(cfg: NetConfig) -> Result<Self> {
        Ok(Self { rt: Some(runtime()?), cfg, host: None, guest: None, flushing: Arc::new(AtomicUsize::new(0)) })
    }

    pub fn config(&self) -> &NetConfig {
        &self.cfg
    }

    /// Identity of this endpoint (host: the identity in the tickets; guest: known after the first `connect_ticket`; it is the
    /// `peer()` the host sees).
    pub fn local_id(&self) -> Option<String> {
        self.host.as_ref().map(|h| &h.endpoint).or(self.guest.as_ref().map(|(_, e)| e)).map(|e| e.id().to_string())
    }

    /// Host side: is a relay connection established? (`false` for a host without relay.)
    pub fn relay_online(&self) -> bool {
        self.host.as_ref().is_some_and(|h| h.endpoint.addr().relay_urls().next().is_some())
    }

    /// Host side: address hints for tickets (relays, then direct addresses). Waits up to `ready_timeout` for them.
    pub fn hints(&self) -> Result<Vec<String>> {
        let host = self.host.as_ref().ok_or(Error::Unexpected("this transport was not created with IrohTransport::host"))?;
        Ok(self.rt().block_on(collect_hints(&host.endpoint, &self.cfg)))
    }

    /// Host side: the hints known right now (no waiting; empty for a guest). They improve while the relay and the local addresses
    /// are discovered: compare with the ones in use to refresh the links.
    pub fn current_hints(&self) -> Vec<String> {
        self.host.as_ref().map(|h| snapshot_hints(&h.endpoint, &self.cfg)).unwrap_or_default()
    }

    /// Host side: the ticket of the link for `role` (identity and hints of this endpoint, key of the role).
    pub fn ticket(&self, keys: &HostKeys, role: Role) -> Result<Ticket> {
        let id = self.local_id().ok_or(Error::Unexpected("this transport was not created with IrohTransport::host"))?;
        keys.ticket(role, &id, &self.hints()?)
    }

    /// Guest side: connects to the host named by the ticket. Fails with `Error::Network` and a cause (`RelayUnreachable`,
    /// `HostUnreachable`, `TimedOut`, `Refused`) no later than `connect_timeout`.
    ///
    /// The relay URLs of a ticket are written by whoever made the link: those that are not `https` on a public address and an
    /// ordinary port are dropped (unless they belong to the configured list, or `allow_private_relays` is set: tests) so that a
    /// link cannot make this machine knock on an internal address. Whether an *unknown* public relay is acceptable is the user's
    /// call (the application asks before dialing).
    pub fn connect_ticket(&mut self, ticket: &Ticket) -> Result<IrohConn> {
        self.connect_addr(self.ticket_addr(ticket)?, None)
    }

    /// The address of a ticket, without the relays this guest may not use (see [`IrohTransport::connect_ticket`]).
    fn ticket_addr(&self, ticket: &Ticket) -> Result<EndpointAddr> {
        let mut addr = ticket_to_addr(ticket)?;
        let before = addr.addrs.len();
        addr.addrs.retain(|a| match a {
            TransportAddr::Relay(url) => self.cfg.allows_ticket_relay(url),
            _ => true,
        });
        if addr.addrs.is_empty() && before > 0 {
            return Err(net(NetCause::HostUnreachable, "the relay address in the link is not allowed (https on a public address is required)"));
        }
        Ok(addr)
    }

    /// Like [`IrohTransport::connect_ticket`], but `cancel` ends the attempt at once (`Error::Closed`).
    pub fn connect_ticket_cancellable(&mut self, ticket: &Ticket, cancel: &CancelToken) -> Result<IrohConn> {
        self.connect_addr(self.ticket_addr(ticket)?, Some(cancel))
    }

    /// Tells the endpoints that the network may have changed (the computer woke up, an interface came back): iroh looks at the
    /// interfaces again, probes the paths of its connections and rebinds its sockets if needed. Harmless when nothing changed; does
    /// not block (the work runs on the transport's runtime).
    pub fn network_change(&self) {
        let endpoints = self.host.iter().map(|h| &h.endpoint).chain(self.guest.iter().map(|(_, e)| e)).cloned();
        for e in endpoints {
            self.rt().spawn(async move { e.network_change().await });
        }
    }

    fn connect_addr(&mut self, mut addr: EndpointAddr, cancel: Option<&CancelToken>) -> Result<IrohConn> {
        let ticket_relays: Vec<RelayUrl> = addr.relay_urls().cloned().collect();
        let relays = GuestRelays::for_ticket(&self.cfg.relays, &ticket_relays);
        if relays == GuestRelays::Off {
            addr.addrs.retain(|a| !a.is_relay());
        }
        if addr.is_empty() {
            return Err(net(NetCause::HostUnreachable, "the link has no usable address (relays are disabled and no direct address is known)"));
        }
        let rt = self.rt.as_ref().expect("the runtime lives as long as the transport");
        let cfg = &self.cfg;
        let endpoint = match &self.guest {
            Some((r, e)) if *r == relays && !e.is_closed() => e.clone(),
            _ => {
                let e = rt.block_on(bind_guest(cfg, &relays))?;
                if let Some((_, old)) = self.guest.replace((relays.clone(), e.clone())) {
                    rt.spawn(async move { old.close().await });
                }
                e
            }
        };
        rt.block_on(dial(&endpoint, addr, cfg, relays != GuestRelays::Off, &self.flushing, cancel))
    }
}

impl Transport for IrohTransport {
    type Conn = IrohConn;

    /// `host` is an endpoint id. The only hints available are the custom relays of the configuration: prefer
    /// [`IrohTransport::connect_ticket`], which uses all the hints of the ticket.
    fn connect(&mut self, host: &str) -> Result<IrohConn> {
        let id: EndpointId = host.parse().map_err(|_| Error::BadTicket("the host identity is not valid".to_owned()))?;
        let relays = match &self.cfg.relays {
            RelaySetting::Custom(urls) => urls.iter().cloned().map(TransportAddr::Relay).collect(),
            _ => Vec::new(),
        };
        self.connect_addr(EndpointAddr::from_parts(id, relays), None)
    }

    fn accept(&mut self) -> Option<IrohConn> {
        self.host.as_mut()?.ready.try_recv().ok()
    }
}

impl Drop for IrohTransport {
    fn drop(&mut self) {
        let Some(rt) = self.rt.take() else { return };
        // Closing gracefully needs `block_on`, which is forbidden inside an async runtime: skip it there.
        if Handle::try_current().is_err() {
            let endpoints: Vec<Endpoint> = self.host.take().map(|h| h.endpoint).into_iter().chain(self.guest.take().map(|(_, e)| e)).collect();
            let flushing = self.flushing.clone();
            rt.block_on(async {
                // connections closed just before (the final `Bye`) get the time to deliver their queue
                timeout(DRAIN_TIMEOUT, async {
                    while flushing.load(Ordering::SeqCst) > 0 {
                        sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .ok();
                for e in endpoints {
                    timeout(Duration::from_secs(2), e.close()).await.ok();
                }
            });
        }
        rt.shutdown_background();
    }
}
