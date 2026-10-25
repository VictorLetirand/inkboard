//! Spike S1 host. Usage:
//! `host --net direct|lan|relay-n0|relay-local|hybrid-n0 [--relay-url URL] [--ticket-file PATH] [--web-url URL]`

use anyhow::{Context, Result, bail};
use s1_iroh::host::{HostNet, spawn_host};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,iroh=warn".into()))
        .init();

    let mut net = String::from("direct");
    let mut relay_url = String::from("http://127.0.0.1:3340");
    let mut ticket_file = None;
    let mut web_url = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().context("missing value");
        match a.as_str() {
            "--net" => net = value()?,
            "--relay-url" => relay_url = value()?,
            "--ticket-file" => ticket_file = Some(value()?),
            "--web-url" => web_url = Some(value()?),
            other => bail!("unknown argument {other}"),
        }
    }
    let net = match net.as_str() {
        "direct" => HostNet::Direct,
        "lan" => HostNet::Lan,
        "relay-n0" => HostNet::RelayN0,
        "relay-local" => HostNet::RelayLocal(relay_url.parse()?),
        "hybrid-n0" => HostNet::HybridN0,
        other => bail!("unknown --net {other}"),
    };

    let host = spawn_host(net.clone()).await?;
    let text = host.ticket.encode()?;
    eprintln!("net={net:?} endpoint ready in {:.0} ms", host.bind_ms);
    eprintln!("addr={:?}", host.ticket.addr);
    println!("{text}");
    if let Some(path) = ticket_file {
        std::fs::write(&path, &text)?;
    }
    if let Some(base) = web_url {
        eprintln!("link: {base}#{text}");
    }
    tokio::signal::ctrl_c().await?;
    host.router.shutdown().await?;
    Ok(())
}
