//! Web side of `session_net.rs` (guest only): `inkboard-net`'s browser transport (iroh through a relay, no direct path) plugged into the
//! guest session (`session_guest`). A page has no thread and no socket: the handshake runs in a `spawn_local` task, the session sees a
//! [`Dialing`] connection that opens when it is done, and the frames are carried by the tasks of `WebConn`.
//!
//! The "Join" window also calls the small settings functions below; a browser keeps no relay file, so the custom relay list is empty.

use crate::join_ui::SessionFactory;
use crate::session_guest::{Clock, ConnectError, Diagnosis, GuestConfig, GuestLink, GuestSession, RelayDiagnoser};
use crate::session_link::{Dialing, connect_error};
use inkboard_net::{CancelToken, NetConfig, WebTransport};
use inkboard_session::crypto::{OsEntropy, Ticket};
use inkboard_session::error::{Error, NetCause};
use inkboard_session::transport::Connection;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;

/// A guest gives up on a handshake after this long (shorter than the session's own 20 s, so that the network says why).
const GUEST_CONNECT: Duration = Duration::from_secs(15);

/// A custom relay list that cannot be used.
#[derive(Debug)]
pub struct RelayListError;

impl RelayListError {
    pub fn message(&self) -> String {
        String::new()
    }
}

/// A browser keeps no relay file: the custom relay list is always empty.
pub fn parse_relay_list(_text: &str) -> Result<Vec<String>, RelayListError> {
    Ok(Vec::new())
}

pub fn read_relays_text(_dir: &Path) -> String {
    String::new()
}

pub fn save_relays_to(_dir: &Path, _urls: &[String]) {}

/// There are no background closings to wait for.
pub fn wait_background_closes(_max: Duration) {}

/* ---------------------------------------------------------------------------------------------------------------- diagnosis */

/// What the last dial found out about the relay (set by the dial, read by the diagnosis).
const RELAY_UNKNOWN: u8 = 0;
const RELAY_REACHED: u8 = 1;
const RELAY_NOT_REACHED: u8 = 2;

/// A page cannot look at the relay (no TCP, and cross-origin answers are opaque): it only knows whether the transport reached it.
/// "Reached" gives the same diagnosis as a relay that answers normally (the host cannot be reached); anything else is generic.
struct WebDiagnoser(Arc<AtomicU8>);

impl RelayDiagnoser for WebDiagnoser {
    fn diagnose(&self, _relay_url: &str) -> Diagnosis {
        let reached = self.0.load(Ordering::SeqCst) == RELAY_REACHED;
        let summary = if reached { "the relay answers (connected by the browser)" } else { "the browser could not connect to the relay" };
        Diagnosis { reachable: reached, summary: summary.to_owned(), ..Diagnosis::default() }
    }
}

/* ------------------------------------------------------------------------------------------------------------------- guest */

/// The link of a guest: dials in a task and returns at once a connection that opens when the handshake is done.
struct WebLink {
    transport: Rc<WebTransport>,
    ticket: Ticket,
    failure: Arc<Mutex<Option<ConnectError>>>,
    relay: Arc<AtomicU8>,
}

impl GuestLink for WebLink {
    /// The ticket carries everything (identity, relays): `host` is only its identity.
    fn connect(&mut self, _host: &str) -> Result<Box<dyn Connection>, ConnectError> {
        let (tx, rx) = channel();
        let cancel = CancelToken::new();
        let (transport, ticket, relay, task_cancel) = (Rc::clone(&self.transport), self.ticket.clone(), Arc::clone(&self.relay), cancel.clone());
        spawn_local(async move {
            let result = transport.connect_ticket_cancellable(&ticket, &task_cancel).await;
            let seen = match &result {
                Err(Error::Network { cause: NetCause::RelayUnreachable | NetCause::Local, .. }) => RELAY_NOT_REACHED,
                _ => RELAY_REACHED,
            };
            relay.store(seen, Ordering::SeqCst);
            let _ = tx.send(result.map_err(|e| connect_error(&e)));
        });
        Ok(Box::new(Dialing::new(self.ticket.host.clone(), rx, Arc::clone(&self.failure), cancel)))
    }

    fn take_failure(&mut self) -> Option<ConnectError> {
        self.failure.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl Drop for WebLink {
    /// Closing the endpoint waits for the last frames (the `Bye` of a guest that leaves): in the background.
    fn drop(&mut self) {
        let transport = Rc::clone(&self.transport);
        spawn_local(async move { transport.close().await });
    }
}

/// The guest factory of the web page (public relays of n0, 15 s handshake). Building a session connects to nothing: the endpoint is
/// created when the session first dials.
pub fn guest_factory() -> SessionFactory {
    Box::new(|ticket, name| {
        let relay = Arc::new(AtomicU8::new(RELAY_UNKNOWN));
        let transport = Rc::new(WebTransport::guest(NetConfig { connect_timeout: GUEST_CONNECT, ..NetConfig::default() }));
        let link = Box::new(WebLink { transport, ticket: ticket.clone(), failure: Arc::default(), relay: Arc::clone(&relay) });
        GuestSession::new(link, Arc::new(WebDiagnoser(relay)), ticket, name, Box::new(OsEntropy), Clock::real(), GuestConfig::default())
    })
}
