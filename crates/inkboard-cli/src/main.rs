//! `ink`: command-line conversion between the whiteboard formats.
//!
//! ```text
//! ink convert schema.mmd schema.drawio
//! ink convert ancien.inkboard.json nouveau.inkboard.json
//! ink info schema.drawio
//! ```

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use inkboard_core::svg::{SvgOptions, export as export_svg};
use inkboard_core::text::ApproxMeasurer;
use inkboard_core::{Document, Kind};
use inkboard_formats::import_any;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "ink", version, about = "Converts whiteboards between formats (draw.io, Excalidraw, JSON Canvas, Mermaid, SVG).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Converts a file. Input: .inkboard.json, .json (prototype), .drawio, .excalidraw, .canvas, .mmd.
    /// Output (by extension): .inkboard.json, .drawio, .excalidraw, .canvas, .svg, .png, .pdf.
    Convert {
        input: PathBuf,
        output: PathBuf,
        /// Page number to export (from 1) for single-page formats.
        #[arg(long, default_value_t = 1)]
        page: usize,
    },
    /// Shows a summary of a file's content.
    Info { input: PathBuf },
    /// Generates a large board (shapes, strokes, bound connectors) to measure performance.
    Bench { count: usize, output: PathBuf },
}

fn read(path: &Path) -> Result<(Document, String)> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let imported = import_any(name, &content, &ApproxMeasurer).with_context(|| format!("opening {name}"))?;
    let summary = imported.report.summary();
    Ok((imported.doc, summary))
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Bench { count, output } => {
            let doc = bench_document(count);
            inkboard_core::serial::save(&doc, &output).with_context(|| format!("writing {}", output.display()))?;
            println!("{} objects → {}", doc.pages[0].objects.len(), output.display());
            Ok(())
        }
        Cmd::Info { input } => {
            let (doc, report) = read(&input)?;
            println!("{} — {} page(s)", doc.name, doc.pages.len());
            for p in &doc.pages {
                let (mut boxes, mut pens, mut conns, mut imgs, mut tables) = (0, 0, 0, 0, 0);
                for o in &p.objects {
                    match o.kind {
                        Kind::Box(_) => boxes += 1,
                        Kind::Pen(_) => pens += 1,
                        Kind::Connector(_) => conns += 1,
                        Kind::Image(_) => imgs += 1,
                        Kind::Table(_) => tables += 1,
                    }
                }
                println!("  \"{}\": {boxes} shape(s), {conns} connector(s), {pens} stroke(s), {imgs} image(s), {tables} table(s)", p.name);
            }
            if !report.is_empty() {
                println!("  note: {report}");
            }
            Ok(())
        }
        Cmd::Convert { input, output, page } => {
            let (doc, report) = read(&input)?;
            let idx = page.checked_sub(1).filter(|i| *i < doc.pages.len()).with_context(|| format!("page {page} does not exist (1 to {})", doc.pages.len()))?;
            let out_name = output.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            let text = if out_name.ends_with(".inkboard.json") || out_name.ends_with(".tableau.json") || out_name.ends_with(".wb") {
                inkboard_core::serial::to_json(&doc, true)
            } else if out_name.ends_with(".drawio") {
                inkboard_formats::drawio::export(&doc, &ApproxMeasurer)
            } else if out_name.ends_with(".excalidraw") {
                inkboard_formats::excalidraw::export(&doc, &doc.pages[idx], &ApproxMeasurer)
            } else if out_name.ends_with(".canvas") {
                let (json, dropped) = inkboard_formats::jsoncanvas::export(&doc.pages[idx]);
                if dropped > 0 {
                    eprintln!("note: {dropped} element(s) that cannot be represented in JSON Canvas were skipped");
                }
                json
            } else if out_name.ends_with(".png") {
                let svg = export_svg(&doc, &doc.pages[idx], None, &ApproxMeasurer, &SvgOptions::default()).context("the page is empty: nothing to export")?;
                let png = inkboard_formats::raster::svg_to_png(&svg, 2.0)?;
                std::fs::write(&output, png).with_context(|| format!("writing {}", output.display()))?;
                println!("{} → {}", input.display(), output.display());
                return Ok(());
            } else if out_name.ends_with(".pdf") {
                let svg = export_svg(&doc, &doc.pages[idx], None, &ApproxMeasurer, &SvgOptions::default()).context("the page is empty: nothing to export")?;
                let pdf = inkboard_formats::raster::svg_to_pdf(&svg)?;
                std::fs::write(&output, pdf).with_context(|| format!("writing {}", output.display()))?;
                println!("{} → {}", input.display(), output.display());
                return Ok(());
            } else if out_name.ends_with(".svg") {
                export_svg(&doc, &doc.pages[idx], None, &ApproxMeasurer, &SvgOptions::default()).context("the page is empty: nothing to export")?
            } else {
                bail!("unrecognized output extension: {out_name} (expected: .inkboard.json, .drawio, .excalidraw, .canvas, .svg, .png, .pdf)");
            };
            std::fs::write(&output, text).with_context(|| format!("writing {}", output.display()))?;
            println!("{} → {}{}", input.display(), output.display(), if report.is_empty() { String::new() } else { format!("  (note: {report})") });
            Ok(())
        }
    }
}

/// Synthetic board: ~60 % labelled shapes, ~25 % strokes, ~15 % bound connectors.
fn bench_document(count: usize) -> Document {
    use inkboard_core::*;
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 10_000) as f64 / 10_000.0
    };
    let mut doc = Document::new("Bench");
    let cols = (count as f64).sqrt().ceil() as usize;
    let mut shapes: Vec<Id> = Vec::new();
    let mut objs: Vec<Object> = Vec::new();
    for i in 0..count {
        let (cx, cy) = ((i % cols) as f64 * 260.0, (i / cols) as f64 * 200.0);
        let kind = rnd();
        if kind < 0.60 {
            let geom = [BoxGeom::Rect, BoxGeom::Ellipse, BoxGeom::Diamond, BoxGeom::Sticky][(rnd() * 4.0) as usize % 4];
            let o = Object::new_box(
                geom,
                Frame::new(cx, cy, 160.0, 90.0),
                Style { fill: Some(style::FILLS[(rnd() * 10.0) as usize % 10]), ..Style::default() },
                Label::new(&format!("Node {i}"), 20.0, Align::Center),
            );
            shapes.push(o.id.clone());
            objs.push(o);
        } else if kind < 0.85 {
            let mut p = Pt::new(cx, cy + 100.0);
            let pts: Vec<Pt> = (0..30)
                .map(|_| {
                    p = Pt::new(p.x + rnd() * 8.0, p.y + (rnd() - 0.5) * 10.0);
                    p
                })
                .collect();
            objs.push(Object::new_pen(pts, false, Style::default()));
        } else if shapes.len() > 2 {
            let a = shapes[shapes.len() - 1].clone();
            // local links, like in a real diagram: to one of the 8 previous shapes
            let b = shapes[shapes.len().saturating_sub(2 + (rnd() * 8.0) as usize) % shapes.len()].clone();
            if a != b {
                objs.push(Object::new(Kind::Connector(Connector::new(End::bound(a), End::bound(b))), Style { width: 2.0, ..Style::default() }));
            }
        }
    }
    doc.pages[0].objects = objs;
    doc
}
