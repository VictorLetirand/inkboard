//! Browser entry point: the page reads the ticket from the URL fragment and calls `run_guest_wasm`.

use n0_future::time::Instant;
use tracing::level_filters::LevelFilter;
use tracing_subscriber_wasm::MakeConsoleWriter;
use wasm_bindgen::prelude::*;

use crate::{Ticket, guest_builder, ms, random_id, run_guest};

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .with_writer(MakeConsoleWriter::default().map_trace_level_to(tracing::Level::DEBUG))
        .without_time()
        .with_ansi(false)
        .init();
}

/// Runs the full guest scenario and returns the report as a JSON string.
#[wasm_bindgen]
pub async fn run_guest_wasm(ticket: String, pings: u32, bytes: u32) -> Result<String, JsError> {
    run(ticket, pings, bytes).await.map_err(|e| JsError::new(&format!("{e:#}")))
}

async fn run(ticket: String, pings: u32, bytes: u32) -> anyhow::Result<String> {
    let ticket = Ticket::decode(&ticket)?;
    let t0 = Instant::now();
    let endpoint = guest_builder(&ticket).bind().await?;
    let bind_ms = ms(t0);
    let report = run_guest(&endpoint, bind_ms, &ticket, random_id(), pings, bytes).await?;
    endpoint.close().await;
    Ok(serde_json::to_string(&report)?)
}
