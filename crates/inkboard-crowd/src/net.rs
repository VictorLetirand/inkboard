//! The real transport for the guests: one iroh endpoint (one identity) per guest, kept for the guest's whole stay.

use crate::link::Dialer;
use inkboard_net::{IrohTransport, NetConfig, RelaySetting};
use inkboard_session::crypto::Ticket;
use inkboard_session::transport::Connection;
use std::time::Duration;

/// How the guests reach the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Same computer, loopback interface, no relay (the host must be started in loopback mode too).
    Loopback,
    /// Direct addresses of the ticket (local network), relays off.
    Lan,
    /// Through the public relays of n0 only, no direct path (be considerate: a few tens of guests at most, small boards).
    Relay,
}

impl Mode {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "loopback" => Some(Self::Loopback),
            "lan" => Some(Self::Lan),
            "relay" => Some(Self::Relay),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Lan => "lan",
            Self::Relay => "relay",
        }
    }

    /// Configuration of a guest endpoint.
    #[must_use]
    pub fn guest_config(self) -> NetConfig {
        let base = NetConfig { connect_timeout: Duration::from_secs(40), ..NetConfig::default() };
        match self {
            Self::Loopback => NetConfig { connect_timeout: Duration::from_secs(40), ..NetConfig::loopback() },
            Self::Lan => NetConfig { relays: RelaySetting::Disabled, ..base },
            Self::Relay => NetConfig { relay_only: true, ..base },
        }
    }

    /// Configuration of the host endpoint.
    #[must_use]
    pub fn host_config(self) -> NetConfig {
        match self {
            Self::Loopback => NetConfig::loopback(),
            Self::Lan => NetConfig { relays: RelaySetting::Disabled, ..NetConfig::default() },
            Self::Relay => NetConfig { relay_only: true, ..NetConfig::default() },
        }
    }
}

/// A guest's way to the host: its own iroh endpoint and the ticket it was given.
#[allow(missing_debug_implementations)]
pub struct IrohDialer {
    transport: IrohTransport,
    ticket: Ticket,
}

impl IrohDialer {
    pub fn new(mode: Mode, ticket: Ticket) -> Result<Self, String> {
        Ok(Self { transport: IrohTransport::guest(mode.guest_config()).map_err(|e| e.to_string())?, ticket })
    }
}

impl Dialer for IrohDialer {
    fn dial(&mut self) -> Result<Box<dyn Connection + Send>, String> {
        self.transport.connect_ticket(&self.ticket).map(|c| Box::new(c) as Box<dyn Connection + Send>).map_err(|e| e.to_string())
    }
}
