//! Reader of the icon library packed by `scripts/build_icon_library.py`.
//!
//! The pack is a small index (sets and icon names) followed by one deflate chunk per set; a chunk is only inflated when one of its icons is
//! needed, and the last few chunks are kept. The icons are SVG documents: white ones (`Set::mono`) are tinted by the application with the
//! stroke color of the object, the others keep their own colors.

use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MAGIC: &[u8; 8] = b"INKLIB01";
/// Number of inflated chunks kept in memory.
const CACHE_CHUNKS: usize = 6;

/// A set of icons (one palette section).
#[derive(Clone, Debug, Deserialize)]
pub struct Set {
    pub id: String,
    /// Section of the palette (AWS, Cisco, Brand logos…).
    pub group: String,
    pub title: String,
    /// White SVG tinted by the application (as opposed to colored icons).
    pub mono: bool,
    pub license: String,
    pub names: Vec<String>,
    offset: usize,
    size: usize,
}

/// An upstream project the library was built from.
#[derive(Clone, Debug, Deserialize)]
pub struct Source {
    pub name: String,
    pub repo: String,
    pub commit: String,
    pub date: String,
    pub license: String,
}

#[derive(Deserialize)]
struct Index {
    version: u32,
    #[serde(default)]
    sources: Vec<Source>,
    sets: Vec<Set>,
}

/// Where an icon is: set index, icon index in the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconRef {
    pub set: usize,
    pub icon: usize,
}

struct Chunk {
    set: usize,
    raw: Vec<u8>,
    /// `(start, end)` of each SVG in `raw`.
    spans: Vec<(usize, usize)>,
}

pub struct Library {
    data: &'static [u8],
    body: usize,
    pub sets: Vec<Set>,
    /// Upstream projects (and the commit of each) the pack was built from.
    pub sources: Vec<Source>,
    cache: Mutex<VecDeque<Arc<Chunk>>>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("sets", &self.sets.len()).field("icons", &self.icon_count()).finish()
    }
}

impl Library {
    /// Opens a pack (normally embedded with `include_bytes!`).
    pub fn open(data: &'static [u8]) -> Result<Self, String> {
        if data.len() < 12 || &data[..8] != MAGIC {
            return Err("not an icon pack".into());
        }
        let n = u32::from_le_bytes(data[8..12].try_into().expect("4 bytes")) as usize;
        let end = 12usize.checked_add(n).filter(|e| *e <= data.len()).ok_or("truncated icon pack")?;
        let json = miniz_oxide::inflate::decompress_to_vec(&data[12..end]).map_err(|e| format!("icon index: {e:?}"))?;
        let index: Index = serde_json::from_slice(&json).map_err(|e| format!("icon index: {e}"))?;
        if index.version != 1 {
            return Err(format!("unsupported icon pack version {}", index.version));
        }
        for s in &index.sets {
            if end.saturating_add(s.offset).saturating_add(s.size) > data.len() {
                return Err(format!("truncated icon pack (set {})", s.id));
            }
        }
        Ok(Self { data, body: end, sets: index.sets, sources: index.sources, cache: Mutex::new(VecDeque::new()) })
    }

    pub fn icon_count(&self) -> usize {
        self.sets.iter().map(|s| s.names.len()).sum()
    }

    /// One line per upstream project: name, short commit and date (for an "about the library" tooltip).
    pub fn sources_summary(&self) -> String {
        self.sources.iter().map(|s| format!("{} {} ({})", s.name, s.commit.get(..7).unwrap_or(&s.commit), s.date)).collect::<Vec<_>>().join(
            "
",
        )
    }

    /// Distinct groups, in the order of their first set.
    pub fn groups(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for s in &self.sets {
            if !out.contains(&s.group.as_str()) {
                out.push(&s.group);
            }
        }
        out
    }

    pub fn name(&self, r: IconRef) -> Option<&str> {
        self.sets.get(r.set)?.names.get(r.icon).map(String::as_str)
    }

    fn chunk(&self, set: usize) -> Option<Arc<Chunk>> {
        let mut cache = self.cache.lock().ok()?;
        if let Some(pos) = cache.iter().position(|c| c.set == set) {
            let c = cache.remove(pos)?;
            cache.push_front(c.clone());
            return Some(c);
        }
        let s = self.sets.get(set)?;
        let start = self.body + s.offset;
        let raw = miniz_oxide::inflate::decompress_to_vec(&self.data[start..start + s.size]).ok()?;
        let mut spans = Vec::with_capacity(s.names.len());
        let mut p = 0usize;
        while p + 4 <= raw.len() {
            let len = u32::from_le_bytes(raw[p..p + 4].try_into().ok()?) as usize;
            let end = (p + 4).checked_add(len).filter(|e| *e <= raw.len())?;
            spans.push((p + 4, end));
            p = end;
        }
        let chunk = Arc::new(Chunk { set, raw, spans });
        cache.push_front(chunk.clone());
        cache.truncate(CACHE_CHUNKS);
        Some(chunk)
    }

    /// SVG document of an icon.
    pub fn svg(&self, r: IconRef) -> Option<String> {
        let chunk = self.chunk(r.set)?;
        let (a, b) = *chunk.spans.get(r.icon)?;
        String::from_utf8(chunk.raw[a..b].to_vec()).ok()
    }

    /// Icons whose name (or set title / group) contains every word of `query` (case-insensitive); name matches first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<IconRef> {
        let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_owned).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut by_name = Vec::new();
        let mut by_set = Vec::new();
        for (si, s) in self.sets.iter().enumerate() {
            let set_text = format!("{} {}", s.group, s.title).to_lowercase();
            for (ii, name) in s.names.iter().enumerate() {
                let lname = name.to_lowercase();
                if words.iter().all(|w| lname.contains(w)) {
                    by_name.push(IconRef { set: si, icon: ii });
                } else if words.iter().all(|w| lname.contains(w) || set_text.contains(w)) {
                    by_set.push(IconRef { set: si, icon: ii });
                }
            }
        }
        by_name.extend(by_set);
        by_name.truncate(limit);
        by_name
    }
}

/// Natural size `(width, height)` of an SVG document, from its `viewBox` or its `width` / `height` attributes.
pub fn svg_dimensions(svg: &str) -> Option<(f64, f64)> {
    let head = &svg[..svg.find('>').map_or(svg.len().min(600), |i| i + 1)];
    let attr = |name: &str| -> Option<&str> {
        let key = format!(" {name}=\"");
        let i = head.find(&key)? + key.len();
        head[i..].split('"').next()
    };
    if let Some(vb) = attr("viewBox") {
        let v: Vec<f64> = vb.split([' ', ',']).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
        if v.len() == 4 && v[2] > 0.0 && v[3] > 0.0 {
            return Some((v[2], v[3]));
        }
    }
    let n = |name: &str| attr(name)?.trim_end_matches("px").parse::<f64>().ok().filter(|v| *v > 0.0);
    Some((n("width")?, n("height")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tiny pack in memory (same layout as the Python script).
    fn pack() -> &'static [u8] {
        fn deflate(d: &[u8]) -> Vec<u8> {
            miniz_oxide::deflate::compress_to_vec(d, 6)
        }
        fn chunk(svgs: &[&str]) -> Vec<u8> {
            let mut raw = Vec::new();
            for s in svgs {
                raw.extend((s.len() as u32).to_le_bytes());
                raw.extend(s.as_bytes());
            }
            deflate(&raw)
        }
        let c1 = chunk(&["<svg id='a'/>", "<svg id='b'/>"]);
        let c2 = chunk(&["<svg id='c'/>"]);
        let index = serde_json::json!({"version": 1, "sets": [
            {"id": "s1", "group": "Cloud", "title": "AWS compute", "mono": false, "license": "x", "offset": 0, "size": c1.len(), "names": ["Lambda", "EC2 instance"]},
            {"id": "s2", "group": "General", "title": "Tabler", "mono": true, "license": "y", "offset": c1.len(), "size": c2.len(), "names": ["Arrow left"]},
        ]});
        let idx = deflate(index.to_string().as_bytes());
        let mut out = MAGIC.to_vec();
        out.extend((idx.len() as u32).to_le_bytes());
        out.extend(idx);
        out.extend(c1);
        out.extend(c2);
        Box::leak(out.into_boxed_slice())
    }

    #[test]
    fn opens_a_pack_and_returns_svgs_in_order() {
        let lib = Library::open(pack()).unwrap();
        assert_eq!(lib.icon_count(), 3);
        assert_eq!(lib.groups(), vec!["Cloud", "General"]);
        assert_eq!(lib.svg(IconRef { set: 0, icon: 1 }).as_deref(), Some("<svg id='b'/>"));
        assert_eq!(lib.svg(IconRef { set: 1, icon: 0 }).as_deref(), Some("<svg id='c'/>"));
        assert_eq!(lib.svg(IconRef { set: 1, icon: 5 }), None);
        assert_eq!(lib.name(IconRef { set: 0, icon: 0 }), Some("Lambda"));
    }

    #[test]
    fn search_matches_names_first_then_set_titles() {
        let lib = Library::open(pack()).unwrap();
        assert_eq!(lib.search("lambda", 10), vec![IconRef { set: 0, icon: 0 }]);
        assert_eq!(lib.search("aws", 10), vec![IconRef { set: 0, icon: 0 }, IconRef { set: 0, icon: 1 }], "set title matches");
        assert_eq!(lib.search("aws instance", 10), vec![IconRef { set: 0, icon: 1 }], "every word must match");
        assert!(lib.search("zzz", 10).is_empty() && lib.search("  ", 10).is_empty());
        assert_eq!(lib.search("a", 1).len(), 1, "limit");
    }

    #[test]
    fn svg_dimensions_come_from_the_view_box_or_the_attributes() {
        assert_eq!(svg_dimensions(r#"<svg xmlns="x" viewBox="-1 -2 30 20" width="5" height="5"><path/></svg>"#), Some((30.0, 20.0)));
        assert_eq!(svg_dimensions(r#"<svg width="64px" height="32"><path/></svg>"#), Some((64.0, 32.0)));
        assert_eq!(svg_dimensions(r#"<svg viewBox="0 0 24 24"/>"#), Some((24.0, 24.0)));
        assert_eq!(svg_dimensions("<svg><path/></svg>"), None);
    }

    #[test]
    fn the_real_pack_names_its_upstream_sources() {
        let lib = Library::open(include_bytes!("../../inkboard-app/assets/icons.bin")).unwrap();
        assert!(lib.sources.len() >= 5, "{:?}", lib.sources);
        assert!(lib.sources.iter().all(|s| s.commit.len() == 40 && s.repo.starts_with("https://github.com/")));
        assert!(lib.sources_summary().contains("draw.io"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Library::open(b"nope").is_err());
        let mut bad = pack().to_vec();
        bad.truncate(30);
        assert!(Library::open(Box::leak(bad.into_boxed_slice())).is_err());
    }

    /// The embedded pack of the application (when it has been built) parses and every icon renders; run with `--ignored`.
    #[test]
    #[ignore]
    fn the_real_pack_is_consistent() {
        let data: &'static [u8] =
            Box::leak(std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../inkboard-app/assets/icons.bin")).unwrap().into_boxed_slice());
        let lib = Library::open(data).unwrap();
        assert!(lib.icon_count() > 20_000);
        for (si, s) in lib.sets.iter().enumerate() {
            for ii in 0..s.names.len() {
                let svg = lib.svg(IconRef { set: si, icon: ii }).unwrap_or_else(|| panic!("{}: icon {ii}", s.id));
                assert!(svg.starts_with("<svg"), "{}: {}", s.id, s.names[ii]);
            }
        }
    }
}
