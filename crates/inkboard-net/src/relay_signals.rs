//! Why can a browser guest not reach its host? The signals a page can observe, and how they are crossed to name a cause.
//!
//! A page cannot read what a security gateway answers (cross-origin answers are opaque) and iroh hides the relay connection behind
//! its own task. Three facts are observable and none of them is enough alone:
//! - the *probe*: a plain HTTPS request (`fetch`, `no-cors`) to the relay: any answer, whatever its status, means the network leads
//!   to the service; an error or silence means it does not;
//! - the *relay link*: whether iroh's endpoint is really connected to a relay (its WebSocket is up), not merely that a relay URL was
//!   selected: `Endpoint::addr()` holds the home relay's URL as soon as it is chosen, connected or not;
//! - the *dial*: how the connection attempt to the host ended.
//!
//! Crossed: nothing passes -> the relay is unreachable; HTTPS passes but iroh never connected -> something lets the page reach the
//! service but not hold a WebSocket to it; iroh is connected and the host still does not answer -> the host is the problem.
//! Pure code (no I/O), so that the table of causes is tested without a browser.

use inkboard_session::error::NetCause;
use iroh::RelayUrl;

/// Outcome of the HTTPS probe of the relay (durations in milliseconds, measured by the caller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Not made (no relay address to probe).
    NotRun,
    /// Some response came back (opaque for a page: status unknown): the network leads to the service.
    Answered { ms: u64 },
    /// The request failed (DNS, refused, certificate rejected, blocked by the browser...): `error` is the browser's own wording.
    Failed { ms: u64, error: String },
    /// No answer within the probe's time-out.
    TimedOut { ms: u64 },
}

impl Probe {
    fn describe(&self) -> String {
        match self {
            Self::NotRun => "not run".to_owned(),
            Self::Answered { ms } => format!("answered (opaque response, status unknown) in {ms} ms"),
            Self::Failed { ms, error } => format!("failed after {ms} ms ({})", clean(error)),
            Self::TimedOut { ms } => format!("timed out after {ms} ms (no answer)"),
        }
    }
}

/// What iroh says about its connection to the relay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelayLink {
    /// The home relay iroh selected, if it selected one yet.
    pub url: Option<String>,
    /// Is the endpoint really connected to it?
    pub connected: bool,
    /// Last connection error reported by iroh (browser wording), when it has one.
    pub last_error: Option<String>,
}

impl RelayLink {
    fn describe(&self) -> String {
        if self.connected {
            return "connected".to_owned();
        }
        let selected = match &self.url {
            Some(u) => format!("home relay selected: {}", clean(u)),
            None => "no home relay selected yet".to_owned(),
        };
        match &self.last_error {
            Some(e) => format!("not connected ({selected}; last error: {})", clean(e)),
            None => format!("not connected ({selected}; no connection error reported)"),
        }
    }
}

/// The cause retained for a failed attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// This computer has no network.
    NoNetwork,
    /// Nothing reaches the relay: blocked, filtered or down.
    RelayUnreachable,
    /// The relay answers an HTTPS request but the page cannot hold a WebSocket to it.
    WebSocketBlocked,
    /// The relay is connected: the host is not there (closed session, offline).
    HostUnreachable,
}

impl Cause {
    /// English, for the diagnostic that is copied.
    pub fn label(self) -> &'static str {
        match self {
            Self::NoNetwork => "no network",
            Self::RelayUnreachable => "relay unreachable",
            Self::WebSocketBlocked => "WebSocket blocked",
            Self::HostUnreachable => "host unreachable",
        }
    }
}

/// Crosses the three signals (see the module documentation). `dial` is the cause the transport gave to the failed attempt.
pub fn conclude(probe: &Probe, link: &RelayLink, dial: NetCause) -> Cause {
    if dial == NetCause::Local {
        return Cause::NoNetwork;
    }
    if link.connected {
        // iroh holds a relay connection: whatever the probe says, the relay is not what stops the host from answering
        return Cause::HostUnreachable;
    }
    match probe {
        Probe::Answered { .. } => Cause::WebSocketBlocked,
        Probe::NotRun | Probe::Failed { .. } | Probe::TimedOut { .. } => Cause::RelayUnreachable,
    }
}

/// Everything observed about one failed attempt, ready to be crossed and to be written in the diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signals {
    pub dial: NetCause,
    pub probe: Probe,
    /// The same probe on the other spelling of the relay's name (with / without the final dot), when there is one.
    pub other_spelling: Option<(String, Probe)>,
    pub link: RelayLink,
    pub user_agent: String,
    /// The relay address as the link carries it.
    pub relay_in_link: String,
    /// The relay address the browser used (see [`without_final_dot`]).
    pub relay_used: String,
    /// How long the attempt lasted before it was given up, in milliseconds.
    pub gave_up_after_ms: u64,
}

impl Signals {
    pub fn cause(&self) -> Cause {
        conclude(&self.probe, &self.link, self.dial)
    }

    /// The observed facts, one line each, for the diagnostic that is copied. No ticket, no key: only addresses of relays and what the
    /// browser reported.
    pub fn facts(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Browser: {}", clean(&self.user_agent)),
            format!("Relay URL in the link: {}", clean(&self.relay_in_link)),
            format!("Relay URL used by the browser: {}", clean(&self.relay_used)),
            format!("HTTPS probe of the relay (GET /generate_204, no-cors): {}", self.probe.describe()),
        ];
        if let Some((url, probe)) = &self.other_spelling {
            lines.push(format!("HTTPS probe, other spelling of the name ({}): {}", clean(url), probe.describe()));
        }
        lines.push(format!("Relay connection (iroh): {}", self.link.describe()));
        lines.push(format!("Gave up after: {:.1} s", self.gave_up_after_ms as f64 / 1000.0));
        lines.push(format!("Cause retained: {}", self.cause().label()));
        lines
    }

    /// One line for the "Diagnosis" entry of the diagnostic.
    pub fn summary(&self) -> &'static str {
        match self.cause() {
            Cause::NoNetwork => "this computer has no network",
            Cause::RelayUnreachable => "the browser reaches neither the relay over HTTPS nor with a connection: the relay seems blocked or down",
            Cause::WebSocketBlocked => "an HTTPS request to the relay is answered, but the browser's WebSocket to it never connected",
            Cause::HostUnreachable => "the browser is connected to the relay, but the host does not answer",
        }
    }
}

/// Text that came from the network or the browser, made safe to show and copy: control characters removed, at most 200 characters.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// The relay address with the final dot of its host name removed (`https://euc1-1.relay.n0.iroh.link./` gives
/// `https://euc1-1.relay.n0.iroh.link/`); unchanged when it has none or cannot be rewritten. iroh's own relay list spells the n0 relays
/// with the final dot (a fully qualified name that skips the search domains of a computer); the dot is legal but unusual in a URL, and
/// the browser guest does not need it: the name is resolved by the browser, which has no search list to skip.
pub fn without_final_dot(url: &RelayUrl) -> RelayUrl {
    let Some(host) = url.host_str() else { return url.clone() };
    let Some(plain) = host.strip_suffix('.').filter(|p| !p.is_empty()) else { return url.clone() };
    // the host is the first occurrence in the text: nothing before it (scheme, `//`, no credentials in a relay address) contains it
    url.as_str().replacen(host, plain, 1).parse().unwrap_or_else(|_| url.clone())
}

/// The same relay address spelled the other way (a final dot added or removed), as text, to probe both spellings.
pub fn other_spelling(url: &RelayUrl) -> Option<String> {
    let host = url.host_str()?;
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return None;
    }
    let flipped = match host.strip_suffix('.') {
        Some(plain) => plain.to_owned(),
        None => format!("{host}."),
    };
    Some(url.as_str().replacen(host, &flipped, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(connected: bool) -> RelayLink {
        RelayLink { url: Some("https://r.example.org/".into()), connected, last_error: (!connected).then(|| "WebSocket error".to_owned()) }
    }
    const ANSWERED: Probe = Probe::Answered { ms: 80 };
    fn failed() -> Probe {
        Probe::Failed { ms: 5, error: "TypeError: Failed to fetch".into() }
    }

    /// The whole table: probe x relay link x dial -> cause.
    #[test]
    fn the_signals_are_crossed_into_one_cause() {
        let timed_out = Probe::TimedOut { ms: 3000 };
        let probes = [ANSWERED, failed(), timed_out, Probe::NotRun];
        for probe in &probes {
            // iroh connected to a relay: the host is the problem, whatever the probe saw
            for dial in [NetCause::HostUnreachable, NetCause::RelayUnreachable, NetCause::TimedOut] {
                assert_eq!(conclude(probe, &link(true), dial), Cause::HostUnreachable, "{probe:?} {dial:?}");
            }
            // no network on the computer: not a matter of relay
            assert_eq!(conclude(probe, &link(false), NetCause::Local), Cause::NoNetwork);
            assert_eq!(conclude(probe, &link(true), NetCause::Local), Cause::NoNetwork);
        }
        for dial in [NetCause::HostUnreachable, NetCause::RelayUnreachable] {
            // nothing passes
            assert_eq!(conclude(&failed(), &link(false), dial), Cause::RelayUnreachable);
            assert_eq!(conclude(&Probe::TimedOut { ms: 1 }, &link(false), dial), Cause::RelayUnreachable);
            assert_eq!(conclude(&Probe::NotRun, &link(false), dial), Cause::RelayUnreachable);
            // HTTPS passes, the relay connection never came up
            assert_eq!(conclude(&ANSWERED, &link(false), dial), Cause::WebSocketBlocked);
        }
        // no relay selected at all is "not connected" too
        assert_eq!(conclude(&ANSWERED, &RelayLink::default(), NetCause::HostUnreachable), Cause::WebSocketBlocked);
        assert_eq!(conclude(&failed(), &RelayLink::default(), NetCause::HostUnreachable), Cause::RelayUnreachable);
    }

    fn signals(probe: Probe, connected: bool) -> Signals {
        Signals {
            dial: NetCause::HostUnreachable,
            probe,
            other_spelling: Some(("https://r.example.org./".into(), ANSWERED)),
            link: link(connected),
            user_agent: "Mozilla/5.0 Edg/130".into(),
            relay_in_link: "https://r.example.org./".into(),
            relay_used: "https://r.example.org/".into(),
            gave_up_after_ms: 15_240,
        }
    }

    #[test]
    fn the_facts_hold_what_was_observed_and_the_cause() {
        let text = signals(ANSWERED, false).facts().join("\n");
        for expected in [
            "Browser: Mozilla/5.0 Edg/130",
            "Relay URL in the link: https://r.example.org./",
            "Relay URL used by the browser: https://r.example.org/",
            "(GET /generate_204, no-cors): answered (opaque response, status unknown) in 80 ms",
            "other spelling of the name (https://r.example.org./): answered",
            "Relay connection (iroh): not connected (home relay selected: https://r.example.org/; last error: WebSocket error)",
            "Gave up after: 15.2 s",
            "Cause retained: WebSocket blocked",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        let text = signals(Probe::TimedOut { ms: 5000 }, false).facts().join("\n");
        assert!(text.contains("timed out after 5000 ms") && text.contains("Cause retained: relay unreachable"), "{text}");
        let text = signals(failed(), true).facts().join("\n");
        assert!(text.contains("failed after 5 ms (TypeError: Failed to fetch)") && text.contains("Relay connection (iroh): connected"), "{text}");
        assert!(text.contains("Cause retained: host unreachable"), "{text}");
    }

    #[test]
    fn what_comes_from_the_browser_is_made_safe_to_copy() {
        let mut s = signals(failed(), false);
        s.user_agent = "Agent\r\nInjected: line".into();
        let text = s.facts().join("\n");
        assert!(text.contains("Browser: AgentInjected: line"), "{text}");
        assert_eq!(text.matches('\n').count(), s.facts().len() - 1, "a control character made an extra line");
    }

    #[test]
    fn the_final_dot_of_a_relay_name_is_removed_and_can_be_probed_both_ways() {
        let dotted: RelayUrl = "https://euc1-1.relay.n0.iroh.link./".parse().unwrap();
        assert_eq!(without_final_dot(&dotted).as_str(), "https://euc1-1.relay.n0.iroh.link/");
        let plain: RelayUrl = "https://euc1-1.relay.n0.iroh.link/".parse().unwrap();
        assert_eq!(without_final_dot(&plain), plain);
        assert_eq!(other_spelling(&dotted).as_deref(), Some("https://euc1-1.relay.n0.iroh.link/"));
        assert_eq!(other_spelling(&plain).as_deref(), Some("https://euc1-1.relay.n0.iroh.link./"));
        let with_port: RelayUrl = "https://relay.example.org.:8443/".parse().unwrap();
        assert_eq!(without_final_dot(&with_port).as_str(), "https://relay.example.org:8443/");
        let ip: RelayUrl = "https://203.0.113.9/".parse().unwrap();
        assert_eq!(without_final_dot(&ip), ip);
        assert_eq!(other_spelling(&ip), None);
    }
}
