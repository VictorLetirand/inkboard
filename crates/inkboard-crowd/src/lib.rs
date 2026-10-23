//! `inkboard-crowd`: load tests of shared sessions.
//!
//! - [`board`]: realistic seed boards of 1 000 / 5 000 / 20 000 objects (shapes, rich texts, code, formulas, icons, connectors, tables,
//!   strokes, about fifty photos, several pages). [`png`]: valid PNGs of a chosen size.
//! - [`guest`] + [`activity`]: a simulated guest = the REAL `inkboard_session::Client` + secure channel on any connection, doing what a
//!   person does (cursor at 30 Hz, strokes, moves, creations, texts, undo, image pastes, arrivals, departures, reconnections).
//! - [`link`]: the clock, the dialer trait and a connection wrapper that degrades a guest's link (latency, loss, bandwidth, freeze).
//! - [`fleet`]: many guests per process on worker threads; [`net`]: the iroh dialer; [`report`], [`stats`], [`proc`], [`digest`]: numbers.
//!
//! The host side lives in the application (`inkboard-app`, test `tests_crowd`), because the session host belongs to the application.
//! See `context/mcp-remote/test-charge.md` for the method and how to replay.

pub mod activity;
pub mod board;
pub mod digest;
pub mod fleet;
pub mod guest;
pub mod link;
pub mod net;
pub mod png;
pub mod proc;
pub mod profile;
pub mod report;
pub mod rng;
pub mod sim;
pub mod stats;
