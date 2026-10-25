//! Fingerprint of a document, to compare the host's board with every guest's ("converged" means the same digest).

use inkboard_core::model::Document;
use sha2::{Digest, Sha256};

/// Hash of the name, background, pages (identifier, name, layers, every object in stacking order). The view of a page is local and
/// ignored, and so are the image bytes (the guests fetch them separately).
pub fn doc_digest(doc: &Document) -> String {
    let mut h = Sha256::new();
    h.update(doc.name.as_bytes());
    h.update(serde_json::to_vec(&doc.background).unwrap_or_default());
    for p in &doc.pages {
        h.update(p.id.0.as_bytes());
        h.update(p.name.as_bytes());
        h.update(serde_json::to_vec(&p.layers).unwrap_or_default());
        for o in &p.objects {
            h.update(serde_json::to_vec(o).unwrap_or_default());
        }
        h.update(b"\x00page\x00");
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
