//! Spike S1 native guest. Usage:
//! `guest <ticket> [--pings N] [--bytes N] [--cut] [--resume HEX_ID]`
//! `--cut` exits the process abruptly (no QUIC close) after the handshake and 50 pings,
//! printing the participant id so that a second run can use `--resume`.

use anyhow::{Context, Result, bail};
use s1_iroh::{Ticket, guest_builder, open_session, ping_bench, random_id, run_guest};

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<[u8; 16]> {
    if s.len() != 32 {
        bail!("participant id must be 32 hex chars");
    }
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16)?;
    }
    Ok(id)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,iroh=warn".into()))
        .init();

    let mut ticket = None;
    let mut pings = 1000u32;
    let mut bytes = 5 * 1024 * 1024u32;
    let mut cut = false;
    let mut participant = random_id();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().context("missing value");
        match a.as_str() {
            "--pings" => pings = value()?.parse()?,
            "--bytes" => bytes = value()?.parse()?,
            "--cut" => cut = true,
            "--ticket-file" => ticket = Some(Ticket::decode(&std::fs::read_to_string(value()?)?)?),
            "--resume" => participant = unhex(&value()?)?,
            t if !t.starts_with("--") => ticket = Some(Ticket::decode(t)?),
            other => bail!("unknown argument {other}"),
        }
    }
    let ticket = ticket.context("ticket argument required")?;

    let t0 = std::time::Instant::now();
    let endpoint = guest_builder(&ticket).bind().await?;
    let bind_ms = t0.elapsed().as_secs_f64() * 1000.0;

    if cut {
        let mut s = open_session(&endpoint, &ticket, participant, "guest-cut").await?;
        ping_bench(&mut s, 50).await?;
        println!("participant={}", hex(&participant));
        eprintln!("cutting abruptly (process exit, no close frame)");
        std::process::exit(0);
    }

    let t1 = std::time::Instant::now();
    let report = run_guest(&endpoint, bind_ms, &ticket, participant, pings, bytes).await?;
    eprintln!("total scenario time {:.0} ms (bind {:.0} ms)", t1.elapsed().as_secs_f64() * 1000.0, bind_ms);
    println!("{}", serde_json::to_string(&report)?);
    endpoint.close().await;
    Ok(())
}
