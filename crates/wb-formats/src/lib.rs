//! `wb-formats` : échanges avec les autres outils.
//!
//! | Format | Import | Export |
//! |---|---|---|
//! | Prototype web (`.tableau.json` v1) | oui | – |
//! | draw.io (`.drawio`, compressé ou non) | oui | oui |
//! | Excalidraw (`.excalidraw`) | oui | oui |
//! | JSON Canvas (`.canvas`) | oui | oui |
//! | Mermaid (flowchart, stateDiagram, sequenceDiagram) | oui | – |

pub mod common;
pub mod drawio;
pub mod excalidraw;
pub mod jsoncanvas;
pub mod legacy;
pub mod math;
pub mod mermaid;
pub mod raster;
mod xml;

pub use common::{FormatError, Imported, Report};

use wb_core::text::Measurer;

/// Formats reconnus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Native,
    Legacy,
    DrawIo,
    Excalidraw,
    JsonCanvas,
    Mermaid,
}

/// Devine le format d'après le nom du fichier et son début.
pub fn detect(file_name: &str, content: &str) -> Option<Format> {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "drawio" => return Some(Format::DrawIo),
        "excalidraw" => return Some(Format::Excalidraw),
        "canvas" => return Some(Format::JsonCanvas),
        "mmd" | "mermaid" => return Some(Format::Mermaid),
        _ => {}
    }
    let head = content.trim_start();
    if head.starts_with('<') {
        return Some(Format::DrawIo);
    }
    if head.starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(content).ok()?;
        if v.get("type").and_then(|t| t.as_str()) == Some("excalidraw") {
            return Some(Format::Excalidraw);
        }
        if v.get("format").and_then(|t| t.as_str()) == Some(wb_core::serial::FORMAT) {
            return Some(Format::Native);
        }
        if v.get("nodes").is_some() && v.get("objects").is_none() {
            return Some(Format::JsonCanvas);
        }
        if v.get("objects").is_some() {
            return Some(Format::Legacy);
        }
        return None;
    }
    let first = head.lines().next().unwrap_or("").trim_start();
    ["flowchart", "graph", "stateDiagram", "sequenceDiagram"].iter().any(|k| first.starts_with(k)).then_some(Format::Mermaid)
}

/// Ouvre un contenu dans n'importe quel format reconnu.
pub fn import_any(file_name: &str, content: &str, m: &dyn Measurer) -> Result<Imported, FormatError> {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let base = base.split('.').next().unwrap_or(base);
    match detect(file_name, content).ok_or(FormatError::Unknown)? {
        Format::Native => {
            Ok(Imported { doc: wb_core::serial::from_json(content).map_err(|e| FormatError::Invalid(e.to_string()))?, report: Report::default() })
        }
        Format::Legacy => legacy::import(content),
        Format::DrawIo => drawio::import(content, base, m),
        Format::Excalidraw => excalidraw::import(content, base, m),
        Format::JsonCanvas => jsoncanvas::import(content, base, m),
        Format::Mermaid => mermaid::import(content, base, m),
    }
}

/// Documents d'exemple utilisés par les tests.
#[cfg(test)]
pub(crate) fn sample_document() -> wb_core::Document {
    use wb_core::*;
    let mut d = wb_core::Document::new("Exemple");
    let mut lab = Label::new("Début\nligne 2", 20.0, Align::Center);
    lab.text.restyle(0, 5, |s| s.bold = true);
    let a = Object::new_box(
        BoxGeom::Rect,
        Frame::new(100.0, 100.0, 160.0, 90.0),
        Style { fill: Some(Color::rgb(165, 216, 255)), stroke: Color::rgb(25, 113, 194), roundness: 0.45, ..Default::default() },
        lab,
    );
    let b = Object::new_box(
        BoxGeom::Ellipse,
        Frame { x: 400.0, y: 100.0, w: 140.0, h: 90.0, angle: 0.3 },
        Style { dash: Dash::Dashed, ..Default::default() },
        Label::new("Fin", 20.0, Align::Center),
    );
    let c = Object::new_box(BoxGeom::Diamond, Frame::new(250.0, 260.0, 140.0, 100.0), Style::default(), Label::new("Choix ?", 20.0, Align::Center));
    let t = Object::new_box(
        BoxGeom::Triangle,
        Frame::new(450.0, 260.0, 100.0, 90.0),
        Style { fill: Some(Color::rgb(255, 201, 201)), ..Default::default() },
        Label::default(),
    );
    let n = Object::new_box(
        BoxGeom::Sticky,
        Frame::new(100.0, 260.0, 130.0, 130.0),
        Style { fill: Some(Color::rgb(255, 216, 168)), width: 0.0, ..Default::default() },
        Label::new("Note", 20.0, Align::Center),
    );
    let mut x = Object::new_box(
        BoxGeom::Text,
        Frame::new(600.0, 100.0, 10.0, 10.0),
        Style { width: 0.0, ..Default::default() },
        Label::new("Titre libre", 28.0, Align::Left),
    );
    if let Kind::Box(bx) = &mut x.kind {
        bx.fit(&ApproxMeasurer);
    }
    let mut conn = Connector::new(End::bound(a.id.clone()), End::bound(b.id.clone()));
    conn.label = Some(Label::new("oui", 14.0, Align::Center));
    let link = Object::new(Kind::Connector(conn), Style { stroke: Color::rgb(224, 49, 49), width: 2.5, ..Default::default() });
    let free = Object::new(Kind::Connector(Connector::new(End::bound(c.id.clone()), End::free(Pt::new(300.0, 480.0)))), Style::default());
    let pen = Object::new_pen(vec![Pt::new(100.0, 450.0), Pt::new(150.0, 470.0), Pt::new(200.0, 440.0), Pt::new(260.0, 480.0)], false, Style::default());
    let aid = d.add_asset("image/png", vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3]);
    let img =
        Object::new(Kind::Image(ImageObj { frame: Frame::new(650.0, 300.0, 80.0, 60.0), asset: aid, latex: None }), Style { width: 0.0, ..Default::default() });
    d.pages[0].objects.extend([a, b, c, t, n, x, link, free, pen, img]);
    d
}
