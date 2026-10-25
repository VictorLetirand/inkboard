//! Web stand-in of `live.rs`: a browser cannot listen on a local port, so there is no live mode of the MCP server.

use crate::app::App;

/// Never created in a browser.
#[derive(Debug)]
pub struct LiveServer;

/// Change reference of the `board_changes` tool (the MCP crate is not part of the web build).
#[derive(Debug)]
pub struct Tracker;

impl App {
    pub fn poll_live(&mut self) {}

    /// Screenshot demo of the agent's cursor (needs the live mode).
    pub fn agent_demo(&mut self) {}
}
