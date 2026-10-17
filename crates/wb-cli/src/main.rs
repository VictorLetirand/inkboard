//! `wb` : conversion en ligne de commande entre les formats du tableau blanc.
//!
//! ```text
//! wb convert schema.mmd schema.drawio
//! wb convert ancien.tableau.json nouveau.tableau.json
//! wb info schema.drawio
//! ```

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use wb_core::svg::{SvgOptions, export as export_svg};
use wb_core::text::ApproxMeasurer;
use wb_core::{Document, Kind};
use wb_formats::import_any;

#[derive(Parser)]
#[command(name = "wb", version, about = "Convertit les tableaux blancs entre formats (draw.io, Excalidraw, JSON Canvas, Mermaid, SVG).")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Convertit un fichier. Entrée : .tableau.json, .json (prototype), .drawio, .excalidraw, .canvas, .mmd.
    /// Sortie (d'après l'extension) : .tableau.json, .drawio, .excalidraw, .canvas, .svg, .png.
    Convert {
        input: PathBuf,
        output: PathBuf,
        /// Numéro de page à exporter (à partir de 1) pour les formats à page unique.
        #[arg(long, default_value_t = 1)]
        page: usize,
    },
    /// Affiche un résumé du contenu d'un fichier.
    Info { input: PathBuf },
    /// Génère un grand tableau (formes, traits, connecteurs liés) pour mesurer les performances.
    Bench { count: usize, output: PathBuf },
}

fn read(path: &Path) -> Result<(Document, String)> {
    let content = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let imported = import_any(name, &content, &ApproxMeasurer).with_context(|| format!("ouverture de {name}"))?;
    let summary = imported.report.summary();
    Ok((imported.doc, summary))
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Bench { count, output } => {
            let doc = bench_document(count);
            wb_core::serial::save(&doc, &output).with_context(|| format!("écriture de {}", output.display()))?;
            println!("{} objets → {}", doc.pages[0].objects.len(), output.display());
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
                println!("  « {} » : {boxes} forme(s), {conns} connecteur(s), {pens} trait(s), {imgs} image(s), {tables} tableau(x)", p.name);
            }
            if !report.is_empty() {
                println!("  à noter : {report}");
            }
            Ok(())
        }
        Cmd::Convert { input, output, page } => {
            let (doc, report) = read(&input)?;
            let idx = page.checked_sub(1).filter(|i| *i < doc.pages.len()).with_context(|| format!("la page {page} n'existe pas (1 à {})", doc.pages.len()))?;
            let out_name = output.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            let text = if out_name.ends_with(".tableau.json") || out_name.ends_with(".wb") {
                wb_core::serial::to_json(&doc, true)
            } else if out_name.ends_with(".drawio") {
                wb_formats::drawio::export(&doc, &ApproxMeasurer)
            } else if out_name.ends_with(".excalidraw") {
                wb_formats::excalidraw::export(&doc, &doc.pages[idx], &ApproxMeasurer)
            } else if out_name.ends_with(".canvas") {
                let (json, dropped) = wb_formats::jsoncanvas::export(&doc.pages[idx]);
                if dropped > 0 {
                    eprintln!("note : {dropped} élément(s) non représentables en JSON Canvas ignorés");
                }
                json
            } else if out_name.ends_with(".png") {
                let svg = export_svg(&doc, &doc.pages[idx], None, &ApproxMeasurer, &SvgOptions::default()).context("la page est vide : rien à exporter")?;
                let png = wb_formats::raster::svg_to_png(&svg, 2.0)?;
                std::fs::write(&output, png).with_context(|| format!("écriture de {}", output.display()))?;
                println!("{} → {}", input.display(), output.display());
                return Ok(());
            } else if out_name.ends_with(".svg") {
                export_svg(&doc, &doc.pages[idx], None, &ApproxMeasurer, &SvgOptions::default()).context("la page est vide : rien à exporter")?
            } else {
                bail!("extension de sortie non reconnue : {out_name} (attendu : .tableau.json, .drawio, .excalidraw, .canvas, .svg, .png)");
            };
            std::fs::write(&output, text).with_context(|| format!("écriture de {}", output.display()))?;
            println!("{} → {}{}", input.display(), output.display(), if report.is_empty() { String::new() } else { format!("  (à noter : {report})") });
            Ok(())
        }
    }
}

/// Tableau synthétique : ~60 % de formes avec texte, ~25 % de traits, ~15 % de connecteurs liés.
fn bench_document(count: usize) -> Document {
    use wb_core::*;
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
                Label::new(&format!("Nœud {i}"), 20.0, Align::Center),
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
            let b = shapes[(rnd() * shapes.len() as f64) as usize % shapes.len()].clone();
            if a != b {
                objs.push(Object::new(Kind::Connector(Connector::new(End::bound(a), End::bound(b))), Style { width: 2.0, ..Style::default() }));
            }
        }
    }
    doc.pages[0].objects = objs;
    doc
}
