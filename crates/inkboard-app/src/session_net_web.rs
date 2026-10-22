//! Web stand-in of `session_net.rs`: the network transport of a browser is a separate piece (it does not use sockets or threads);
//! until it is plugged in, only the small settings functions the "Join" window calls exist.

use std::path::Path;
use std::time::Duration;

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
