//! What the native (`session_net`) and the browser (`session_net_web`) guest links have in common: a connection that is still being
//! dialed in the background (a thread, or a browser task), and the mapping of a transport error to the failure classes of the guest.

use crate::session_guest::{ConnectError, ConnectErrorKind};
use inkboard_net::CancelToken;
use inkboard_session::error::{Error, NetCause, Result};
use inkboard_session::transport::Connection;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

/// Frames a connection that is still being established holds back (the `Hello` and what follows).
const MAX_QUEUED: usize = 1024;

pub fn connect_error(e: &Error) -> ConnectError {
    let kind = match e {
        Error::Network { cause: NetCause::Local, .. } => ConnectErrorKind::NoNetwork,
        Error::Network { cause: NetCause::TimedOut, .. } => ConnectErrorKind::TimedOut,
        _ => ConnectErrorKind::Other,
    };
    ConnectError::new(kind, &e.to_string())
}

/// A connection being established: frames sent meanwhile are kept and go out once it is open; if it never opens, the reason is left
/// for [`crate::session_guest::GuestLink::take_failure`].
pub struct Dialing<C> {
    peer: String,
    rx: Option<Receiver<std::result::Result<C, ConnectError>>>,
    conn: Option<C>,
    queued: Vec<Vec<u8>>,
    dead: bool,
    failure: Arc<Mutex<Option<ConnectError>>>,
    /// Ends the dial running in the background (closing, or dropping, this connection really stops the attempt).
    cancel: CancelToken,
}

impl<C> Dialing<C> {
    pub fn new(peer: String, rx: Receiver<std::result::Result<C, ConnectError>>, failure: Arc<Mutex<Option<ConnectError>>>, cancel: CancelToken) -> Self {
        Self { peer, rx: Some(rx), conn: None, queued: Vec::new(), dead: false, failure, cancel }
    }
}

impl<C> Drop for Dialing<C> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl<C: Connection> Dialing<C> {
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
            Err(TryRecvError::Disconnected) => self.fail(ConnectError::new(ConnectErrorKind::Other, "the connection task ended without an answer")),
        }
    }

    fn fail(&mut self, e: ConnectError) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
        self.dead = true;
        self.rx = None;
        self.queued.clear();
    }
}

impl<C: Connection> Connection for Dialing<C> {
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
        self.cancel.cancel();
        if let Some(c) = self.conn.as_mut() {
            c.close();
        }
        self.dead = true;
        self.rx = None;
        self.queued.clear();
    }
}
