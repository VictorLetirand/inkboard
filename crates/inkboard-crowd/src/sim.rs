//! The simulated in-memory network as a way for the guests to reach the host (deterministic runs, UI benches).

use crate::link::Dialer;
use inkboard_session::crypto::Ticket;
use inkboard_session::sim::SimTransport;
use inkboard_session::transport::{Connection, Transport};

#[derive(Debug)]
pub struct SimDialer {
    transport: SimTransport,
    host: String,
    #[allow(dead_code)]
    ticket: Ticket,
}

impl SimDialer {
    pub fn new(transport: SimTransport, host: &str, ticket: Ticket) -> Self {
        Self { transport, host: host.to_owned(), ticket }
    }
}

impl Dialer for SimDialer {
    fn dial(&mut self) -> Result<Box<dyn Connection + Send>, String> {
        self.transport.connect(&self.host).map(|c| Box::new(c) as Box<dyn Connection + Send>).map_err(|e| e.to_string())
    }

    fn instant(&self) -> bool {
        true
    }
}
