//! Web side of `session_net.rs` (guest only): `inkboard-net`'s browser transport (iroh through a relay, no direct path) plugged into the
//! guest session (`session_guest`). A page has no thread and no socket: the handshake runs in a `spawn_local` task, the session sees a
//! [`Dialing`] connection that opens when it is done, and the frames are carried by the tasks of `WebConn`.
//!
//! The "Join" window also calls the small settings functions below; a browser keeps no relay file, so the custom relay list is empty.

use crate::join_ui::SessionFactory;
use crate::session_guest::{Clock, ConnectError, Diagnosis, GuestConfig, GuestLink, GuestSession, NetWatch, RelayDiagnoser};
use crate::session_link::{Dialing, connect_error};
use inkboard_net::relay_signals::{Cause, Signals};
use inkboard_net::{CancelToken, NetConfig, WebTransport};
use inkboard_session::crypto::{OsEntropy, Ticket};
use inkboard_session::transport::Connection;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;

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

/// What the last dial observed about the relay (set by the dial task, read by the diagnosis): `None` until a dial failed on the network.
type Observed = Arc<Mutex<Option<Signals>>>;

/// A page cannot read what the relay (or a gateway in front of it) answers: cross-origin answers are opaque. It can tell whether an
/// HTTPS request gets through and whether iroh is really connected to the relay; the two crossed give the cause (see
/// `inkboard_net::relay_signals`). "The relay answers" is only said when iroh holds a relay connection.
struct WebDiagnoser(Observed);

impl RelayDiagnoser for WebDiagnoser {
    fn diagnose(&self, _relay_url: &str) -> Diagnosis {
        let Some(signals) = self.0.lock().unwrap_or_else(PoisonError::into_inner).clone() else {
            return Diagnosis { summary: "the browser could not connect to the relay".to_owned(), ..Diagnosis::default() };
        };
        let cause = signals.cause();
        Diagnosis {
            reachable: cause == Cause::HostUnreachable,
            websocket_blocked: cause == Cause::WebSocketBlocked,
            facts: signals.facts(),
            summary: signals.summary().to_owned(),
            ..Diagnosis::default()
        }
    }
}

/* ------------------------------------------------------------------------------------------------------- timings (bench knob) */

static CONNECT_MS: AtomicU64 = AtomicU64::new(15_000);
static PROBE_MS: AtomicU64 = AtomicU64::new(5_000);
static PROBE_DELAY_MS: AtomicU64 = AtomicU64::new(2_000);

/// Test hook of the browser bench (`web_test`): shorter time-outs, so that the silent-block scenarios do not take minutes.
#[cfg(feature = "web-test")]
pub fn set_timeouts(connect_ms: u64, probe_ms: u64, probe_delay_ms: u64) {
    CONNECT_MS.store(connect_ms, Ordering::SeqCst);
    PROBE_MS.store(probe_ms, Ordering::SeqCst);
    PROBE_DELAY_MS.store(probe_delay_ms, Ordering::SeqCst);
}

fn net_config() -> NetConfig {
    NetConfig {
        connect_timeout: Duration::from_millis(CONNECT_MS.load(Ordering::SeqCst)),
        probe_timeout: Duration::from_millis(PROBE_MS.load(Ordering::SeqCst)),
        probe_delay: Duration::from_millis(PROBE_DELAY_MS.load(Ordering::SeqCst)),
        ..NetConfig::default()
    }
}

/* ------------------------------------------------------------------------------------------------------------------- guest */

/// The link of a guest: dials in a task and returns at once a connection that opens when the handshake is done.
struct WebLink {
    transport: Rc<WebTransport>,
    ticket: Ticket,
    failure: Arc<Mutex<Option<ConnectError>>>,
    observed: Observed,
}

impl GuestLink for WebLink {
    /// The ticket carries everything (identity, relays): `host` is only its identity.
    fn connect(&mut self, _host: &str) -> Result<Box<dyn Connection>, ConnectError> {
        let (tx, rx) = channel();
        let cancel = CancelToken::new();
        let (transport, ticket, observed, task_cancel) = (Rc::clone(&self.transport), self.ticket.clone(), Arc::clone(&self.observed), cancel.clone());
        spawn_local(async move {
            let (result, signals) = transport.connect_ticket_observed(&ticket, &task_cancel).await;
            // stored before the failure is reported: the diagnosis (asked for when the failure arrives) reads it
            *observed.lock().unwrap_or_else(PoisonError::into_inner) = signals;
            let _ = tx.send(result.map_err(|e| connect_error(&e)));
        });
        Ok(Box::new(Dialing::new(self.ticket.host.clone(), rx, Arc::clone(&self.failure), cancel)))
    }

    fn take_failure(&mut self) -> Option<ConnectError> {
        self.failure.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// The browser's relay connection is probed again now (see [`BrowserNetWatch`]).
    fn network_changed(&mut self) {
        self.transport.network_change();
    }
}

/// The `online` and `visibilitychange` events of the browser (listened to by `platform`): the network came back, or the tab is awake
/// again after a time without frames. The guest then reconnects at once instead of waiting for its pauses between attempts.
struct BrowserNetWatch;

impl NetWatch for BrowserNetWatch {
    fn changed(&mut self) -> bool {
        crate::platform::take_net_event()
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
        let observed = Observed::default();
        let transport = Rc::new(WebTransport::guest(net_config()));
        let link = Box::new(WebLink { transport, ticket: ticket.clone(), failure: Arc::default(), observed: Arc::clone(&observed) });
        let mut session = GuestSession::new(link, Arc::new(WebDiagnoser(observed)), ticket, name, Box::new(OsEntropy), Clock::real(), GuestConfig::default());
        session.set_net_watch(Box::new(BrowserNetWatch));
        session
    })
}
