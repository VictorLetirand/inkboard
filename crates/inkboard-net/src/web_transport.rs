//! `inkboard_session::transport::Connection` over iroh in a browser (guest side only).
//!
//! Same wire format as the native transport (see [`crate::wire`]): one QUIC connection, one bidirectional stream opened by the guest
//! with the `IBN1` preamble, then `u32 length | bytes` frames. A browser has no UDP: iroh reaches the host through a relay only (the
//! WebSocket to the relay is iroh's own), and everything is single-threaded. So:
//! - [`WebTransport::connect_ticket`] is `async` (a page cannot block); the application awaits it in a `spawn_local` task.
//! - A connection is [`WebConn`]: `send`/`recv` never block, the buffers are fed by one reader task and one writer task
//!   (`wasm_bindgen_futures::spawn_local`), with the same limits as the native one (frame count and bytes, `Congested`).
//! - Errors carry the same [`NetCause`]s as natively, so the interface shows the same diagnosis: `RelayUnreachable` (no relay
//!   connection: blocked, offline or a security gateway), `HostUnreachable`, `TimedOut` (120 s idle timeout, `NetConfig`), `Refused`;
//!   an invalid ticket is `Error::BadTicket` (see `ticket_to_addr`).

use crate::cancel::CancelToken;
use crate::config::NetConfig;
use crate::wire::{
    CLOSE_NORMAL, FLUSH_TIMEOUT, FrameError, GuestRelays, MAX_QUEUED_BYTES, QUEUE, bind_guest, dial_failure, dial_plan, net, open_stream, read_fault,
    read_frame, ticket_addr, write_fault, write_frame,
};
use inkboard_session::crypto::Ticket;
use inkboard_session::error::{Error, NetCause, Result};
use inkboard_session::limits::Violation;
use inkboard_session::transport::Connection;
use iroh::Endpoint;
use iroh::endpoint::{Connection as QuicConn, RecvStream, SendStream, VarInt};
use n0_future::future::or;
use n0_future::time::{sleep, timeout};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use wasm_bindgen_futures::spawn_local;

/// How long closing the endpoint waits for the last frames to be delivered.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// State shared by a connection handle and its two tasks.
struct Shared {
    open: Cell<bool>,
    /// Why the connection ended, when it is not a plain close.
    fault: RefCell<Option<Error>>,
    /// Set when the peer is gone (end of stream or error): tells the writer to stop.
    gone: watch::Sender<bool>,
    /// Bytes waiting in the outgoing queue / in the inbox (see `MAX_QUEUED_BYTES`).
    out_bytes: Cell<usize>,
    in_bytes: Cell<usize>,
}

impl Shared {
    fn new() -> Rc<Self> {
        Rc::new(Self { open: Cell::new(true), fault: RefCell::new(None), gone: watch::channel(false).0, out_bytes: Cell::new(0), in_bytes: Cell::new(0) })
    }

    /// The first call wins and records the cause.
    fn mark_closed(&self, fault: Option<Error>) {
        if self.open.replace(false) {
            *self.fault.borrow_mut() = fault;
        }
    }

    fn peer_gone(&self, fault: Option<Error>) {
        self.mark_closed(fault);
        self.gone.send_replace(true);
    }
}

/// One connection to the host. Frames received before the end of the connection stay readable with `recv`.
pub struct WebConn {
    peer: String,
    out: Option<mpsc::Sender<Vec<u8>>>,
    inbox: mpsc::Receiver<Vec<u8>>,
    shared: Rc<Shared>,
    max_frame: usize,
}

impl std::fmt::Debug for WebConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebConn").field("peer", &self.peer).field("open", &self.is_open()).finish_non_exhaustive()
    }
}

impl WebConn {
    /// Starts the two tasks.
    fn start(conn: QuicConn, send: SendStream, recv: RecvStream, max_frame: usize) -> Self {
        let shared = Shared::new();
        let (out_tx, out_rx) = mpsc::channel(QUEUE);
        let (in_tx, in_rx) = mpsc::channel(QUEUE);
        spawn_local(read_loop(recv, in_tx, shared.clone(), conn.clone(), max_frame));
        spawn_local(write_loop(send, out_rx, shared.clone(), conn.clone()));
        Self { peer: conn.remote_id().to_string(), out: Some(out_tx), inbox: in_rx, shared, max_frame }
    }

    /// Why the connection is closed: `None` while open; `Error::Closed` for a plain close, otherwise the cause
    /// (`Network { TimedOut | Refused | .. }`).
    pub fn close_cause(&self) -> Option<Error> {
        if self.shared.open.get() {
            return None;
        }
        Some(self.shared.fault.borrow().clone().unwrap_or(Error::Closed))
    }
}

impl Connection for WebConn {
    fn peer(&self) -> &str {
        &self.peer
    }

    fn send(&mut self, frame: &[u8]) -> Result<()> {
        if frame.len() > self.max_frame {
            return Err(Violation::MessageTooLarge { size: frame.len(), max: self.max_frame }.into());
        }
        let Some(out) = self.out.as_ref().filter(|_| self.is_open()) else { return Err(self.close_cause().unwrap_or(Error::Closed)) };
        let queued = &self.shared.out_bytes;
        if queued.get() + frame.len() > MAX_QUEUED_BYTES {
            return Err(net(NetCause::Congested, "the outgoing queue holds too many bytes"));
        }
        queued.set(queued.get() + frame.len());
        match out.try_send(frame.to_vec()) {
            Ok(()) => Ok(()),
            Err(e) => {
                queued.set(queued.get() - frame.len());
                match e {
                    mpsc::error::TrySendError::Full(_) => Err(net(NetCause::Congested, "the outgoing queue is full")),
                    mpsc::error::TrySendError::Closed(_) => Err(self.close_cause().unwrap_or(Error::Closed)),
                }
            }
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        let frame = self.inbox.try_recv().ok()?;
        self.shared.in_bytes.set(self.shared.in_bytes.get() - frame.len());
        Some(frame)
    }

    fn is_open(&self) -> bool {
        self.shared.open.get()
    }

    /// Flushes the queued frames in the background, then closes the connection.
    fn close(&mut self) {
        self.shared.mark_closed(None);
        self.out = None;
    }
}

impl Drop for WebConn {
    fn drop(&mut self) {
        self.close();
    }
}

async fn read_loop(mut recv: RecvStream, inbox: mpsc::Sender<Vec<u8>>, shared: Rc<Shared>, conn: QuicConn, max_frame: usize) {
    loop {
        // back-pressure by bytes: the application is not draining what is already here
        while shared.in_bytes.get() >= MAX_QUEUED_BYTES {
            if inbox.is_closed() {
                return;
            }
            sleep(Duration::from_millis(5)).await;
        }
        match read_frame(&mut recv, max_frame).await {
            // `send` fails only when the handle is gone; the writer then flushes and closes.
            Ok(Some(frame)) => {
                shared.in_bytes.set(shared.in_bytes.get() + frame.len());
                if inbox.send(frame).await.is_err() {
                    break;
                }
            }
            Ok(None) => return shared.peer_gone(None),
            Err(FrameError::Read(e)) => return shared.peer_gone(Some(read_fault(&e, &conn))),
            Err(FrameError::Truncated) => return shared.peer_gone(None),
            Err(FrameError::TooLarge(size)) => {
                shared.peer_gone(Some(Violation::MessageTooLarge { size, max: max_frame }.into()));
                conn.close(VarInt::from_u32(crate::wire::CLOSE_REFUSED), b"frame too large");
                return;
            }
        }
    }
}

async fn write_loop(mut send: SendStream, mut queue: mpsc::Receiver<Vec<u8>>, shared: Rc<Shared>, conn: QuicConn) {
    let mut gone = shared.gone.subscribe();
    // `true` when the local side asked to close: then the queue is drained first and the peer given time to read it.
    let local_close = loop {
        let next = or(async { Some(queue.recv().await) }, async {
            gone.wait_for(|g| *g).await.ok();
            None
        })
        .await;
        match next {
            None => break false,
            Some(None) => break true,
            Some(Some(frame)) => {
                shared.out_bytes.set(shared.out_bytes.get() - frame.len());
                if let Err(e) = write_frame(&mut send, &frame).await {
                    shared.peer_gone(Some(write_fault(&e, &conn)));
                    break false;
                }
            }
        }
    };
    send.finish().ok();
    if local_close {
        // The side that sends last must not close before the peer has read everything: wait for the peer to close.
        timeout(FLUSH_TIMEOUT, conn.closed()).await.ok();
    }
    conn.close(VarInt::from_u32(CLOSE_NORMAL), b"bye");
}

/// Browser transport of a guest: one iroh endpoint (relay only), kept for the whole session so that the host recognizes the guest
/// when it reconnects. Share it with an `Rc`; every method takes `&self`.
pub struct WebTransport {
    cfg: NetConfig,
    guest: RefCell<Option<(GuestRelays, Endpoint)>>,
}

impl std::fmt::Debug for WebTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransport").field("bound", &self.guest.borrow().is_some()).finish_non_exhaustive()
    }
}

impl WebTransport {
    /// Nothing is bound until `connect_ticket`.
    pub fn guest(cfg: NetConfig) -> Self {
        Self { cfg, guest: RefCell::new(None) }
    }

    pub fn config(&self) -> &NetConfig {
        &self.cfg
    }

    /// Identity of this endpoint, known after the first `connect_ticket` (it is the `peer()` the host sees).
    pub fn local_id(&self) -> Option<String> {
        self.guest.borrow().as_ref().map(|(_, e)| e.id().to_string())
    }

    /// Connects to the host named by the ticket. Fails with `Error::Network` and a cause (`RelayUnreachable`, `HostUnreachable`,
    /// `TimedOut`, `Refused`) no later than `connect_timeout`, or with `Error::BadTicket`. The relays of the ticket are filtered as
    /// natively (https on a public address and an ordinary port, unless configured).
    pub async fn connect_ticket(&self, ticket: &Ticket) -> Result<WebConn> {
        self.dial(ticket, None).await
    }

    /// Like [`WebTransport::connect_ticket`], but `cancel` ends the attempt at once (`Error::Closed`).
    pub async fn connect_ticket_cancellable(&self, ticket: &Ticket, cancel: &CancelToken) -> Result<WebConn> {
        self.dial(ticket, Some(cancel)).await
    }

    /// Closes the endpoint (the frames of the connections closed just before are given a moment to go out).
    pub async fn close(&self) {
        let endpoint = self.guest.borrow_mut().take();
        if let Some((_, e)) = endpoint {
            timeout(CLOSE_TIMEOUT, e.close()).await.ok();
        }
    }

    async fn dial(&self, ticket: &Ticket, cancel: Option<&CancelToken>) -> Result<WebConn> {
        let (addr, relays) = dial_plan(&self.cfg, ticket_addr(&self.cfg, ticket)?)?;
        let relays_on = relays != GuestRelays::Off;
        let endpoint = self.endpoint(&relays).await?;
        let attempt = async { Some(timeout(self.cfg.connect_timeout, open_stream(&endpoint, addr, relays_on)).await) };
        let aborted = async {
            match cancel {
                Some(c) => c.cancelled().await,
                None => std::future::pending().await,
            }
            None
        };
        match or(attempt, aborted).await {
            None => Err(Error::Closed),
            Some(Ok(Ok((conn, send, recv)))) => Ok(WebConn::start(conn, send, recv, self.cfg.max_frame)),
            Some(Ok(Err(e))) => Err(e),
            Some(Err(_)) => Err(dial_failure(&endpoint, relays_on, format!("no answer within {} s", self.cfg.connect_timeout.as_secs_f32()))),
        }
    }

    /// The endpoint for these relays: the one of the previous dial when it is still good, otherwise a new one.
    async fn endpoint(&self, relays: &GuestRelays) -> Result<Endpoint> {
        let current = |slot: &Option<(GuestRelays, Endpoint)>| slot.as_ref().filter(|(r, e)| r == relays && !e.is_closed()).map(|(_, e)| e.clone());
        if let Some(e) = current(&self.guest.borrow()) {
            return Ok(e);
        }
        let fresh = bind_guest(&self.cfg, relays).await?;
        let mut slot = self.guest.borrow_mut();
        // another dial may have bound one while this one was binding: keep that one
        if let Some(e) = current(&slot) {
            spawn_local(async move { fresh.close().await });
            return Ok(e);
        }
        if let Some((_, old)) = slot.replace((relays.clone(), fresh.clone())) {
            spawn_local(async move { old.close().await });
        }
        Ok(fresh)
    }
}
