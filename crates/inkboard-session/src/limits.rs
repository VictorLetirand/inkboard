//! Validation of everything a guest sends to the host: pure functions with configurable limits.
//!
//! The host calls these before applying or relaying anything; a violation is reported to the guest (and repeated violations get
//! the guest excluded, which is the job of the host manager, not of this crate).

use crate::proto::{AssetChunk, AssetInfo, Message};
use crate::wire::WireOp;
use inkboard_core::geom::{Frame, Pt};
use inkboard_core::model::{Id, Kind, Object, Page};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Violation {
    #[error("message too large ({size} bytes, maximum {max})")]
    MessageTooLarge { size: usize, max: usize },
    #[error("too many {what} in one message ({count}, maximum {max})")]
    TooMany { what: &'static str, count: usize, max: usize },
    #[error("{field} is out of bounds or not a finite number (maximum magnitude {max})")]
    OutOfBounds { field: &'static str, max: u64 },
    #[error("text too long ({len} characters, maximum {max})")]
    TextTooLong { len: usize, max: usize },
    #[error("identifier empty or too long")]
    BadId,
    #[error("malformed object: {0}")]
    Malformed(&'static str),
    #[error("image too large ({size} bytes, maximum {max})")]
    ImageTooLarge { size: u64, max: u64 },
    #[error("upload quota exceeded ({used} + {size} bytes, maximum {max})")]
    QuotaExceeded { used: u64, size: u64, max: u64 },
    #[error("sending too fast ({what})")]
    RateExceeded { what: &'static str },
    /// A guest touched something the host locked (or tried to lock or unlock it).
    #[error("{0}")]
    Locked(&'static str),
}

/// Configurable limits (see `PLAN_REMOTE.md` section 4).
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    pub max_message_bytes: usize,
    pub max_ops_per_message: usize,
    pub max_objects_per_message: usize,
    pub max_ids_per_message: usize,
    /// Identifiers of one `Order` operation: it lists the whole stacking order of a page, so it must allow the biggest page (it stays
    /// under `max_message_bytes`: about 15 bytes per identifier).
    pub max_order_ids: usize,
    /// Maximum magnitude of any number (positions, sizes...).
    pub max_abs_value: f64,
    pub max_object_size: f64,
    pub max_stroke_width: f64,
    pub max_font_size: f64,
    /// Total characters of all the strings of one object.
    pub max_text_chars: usize,
    pub max_name_chars: usize,
    pub max_id_len: usize,
    pub max_pen_points: usize,
    pub max_table_dim: usize,
    pub max_pages: usize,
    pub max_layers: usize,
    pub max_image_bytes: u64,
    pub max_asset_chunk_bytes: usize,
    pub guest_asset_quota_bytes: u64,
    pub ops_per_sec: u32,
    pub ops_burst: u32,
    pub transient_per_sec: u32,
    pub transient_burst: u32,
    /// `Resync` and repeated `Hello` of one participant (each can make the host send the whole document).
    pub sync_per_sec: u32,
    pub sync_burst: u32,
    /// Frames and bytes one participant may send, checked before any decoding.
    pub frames_per_sec: u32,
    pub frames_burst: u32,
    pub bytes_per_sec: u32,
    pub bytes_burst: u32,
    /// Bytes of ephemeral messages (presence, transient states, stroke points) one participant may send; the host relays them to everybody.
    pub ephemeral_bytes_per_sec: u32,
    pub ephemeral_bytes_burst: u32,
}

impl Limits {
    /// Bounds for what the host says: the same numeric and structural checks as for a guest (nothing non-finite, nothing huge), but
    /// with the counts lifted, since the host legitimately sends whole pages or deletes thousands of objects in one modification.
    pub fn host_trust() -> Self {
        Self {
            max_message_bytes: 16 << 20,
            max_ops_per_message: 10_000,
            max_objects_per_message: 100_000,
            max_ids_per_message: 200_000,
            max_order_ids: 200_000,
            max_text_chars: 1_000_000,
            max_pen_points: 500_000,
            max_pages: 1_000,
            max_layers: 1_000,
            ..Self::default()
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_message_bytes: 1 << 20,
            max_ops_per_message: 200,
            max_objects_per_message: 500,
            max_ids_per_message: 5_000,
            max_order_ids: 60_000,
            max_abs_value: 1e7,
            max_object_size: 1e6,
            max_stroke_width: 1_000.0,
            max_font_size: 2_000.0,
            max_text_chars: 100_000,
            max_name_chars: 200,
            max_id_len: 64,
            max_pen_points: 50_000,
            max_table_dim: 200,
            max_pages: 100,
            max_layers: 100,
            max_image_bytes: 15 << 20,
            max_asset_chunk_bytes: 256 << 10,
            guest_asset_quota_bytes: 100 << 20,
            ops_per_sec: 50,
            ops_burst: 100,
            transient_per_sec: 60,
            transient_burst: 120,
            sync_per_sec: 1,
            sync_burst: 3,
            frames_per_sec: 400,
            frames_burst: 800,
            bytes_per_sec: 8 << 20,
            bytes_burst: 16 << 20,
            ephemeral_bytes_per_sec: 2 << 20,
            ephemeral_bytes_burst: 4 << 20,
        }
    }
}

fn too_many(what: &'static str, count: usize, max: usize) -> Result<(), Violation> {
    if count > max { Err(Violation::TooMany { what, count, max }) } else { Ok(()) }
}

/// Size of a raw frame or message, checked before any decoding.
pub fn check_message_size(l: &Limits, size: usize) -> Result<(), Violation> {
    if size > l.max_message_bytes { Err(Violation::MessageTooLarge { size, max: l.max_message_bytes }) } else { Ok(()) }
}

fn check_num(l: &Limits, field: &'static str, v: f64) -> Result<(), Violation> {
    if v.is_finite() && v.abs() <= l.max_abs_value { Ok(()) } else { Err(Violation::OutOfBounds { field, max: l.max_abs_value as u64 }) }
}

fn check_len(field: &'static str, v: f64, max: f64, allow_zero: bool) -> Result<(), Violation> {
    if v.is_finite() && v <= max && (v > 0.0 || (allow_zero && v == 0.0)) { Ok(()) } else { Err(Violation::OutOfBounds { field, max: max as u64 }) }
}

pub fn check_pt(l: &Limits, field: &'static str, p: &Pt) -> Result<(), Violation> {
    check_num(l, field, p.x)?;
    check_num(l, field, p.y)
}

fn check_frame(l: &Limits, f: &Frame) -> Result<(), Violation> {
    check_num(l, "position", f.x)?;
    check_num(l, "position", f.y)?;
    check_len("size", f.w, l.max_object_size, true)?;
    check_len("size", f.h, l.max_object_size, true)?;
    check_num(l, "angle", f.angle)
}

pub fn check_id(l: &Limits, id: &Id) -> Result<(), Violation> {
    if id.0.is_empty() || id.0.len() > l.max_id_len { Err(Violation::BadId) } else { Ok(()) }
}

pub fn check_name(l: &Limits, name: &str) -> Result<(), Violation> {
    let len = name.chars().count();
    if len > l.max_name_chars { Err(Violation::TextTooLong { len, max: l.max_name_chars }) } else { Ok(()) }
}

/// Walks the serialized form of the parts that have no typed check (style, connector, rich text): every number is bounded, widths
/// are bounded as stroke widths, and the strings are counted.
fn walk(l: &Limits, v: &Value, key: &str, depth: usize, chars: &mut usize) -> Result<(), Violation> {
    if depth > 32 {
        return Err(Violation::Malformed("nesting too deep"));
    }
    match v {
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::INFINITY);
            if key.contains("width") { check_len("stroke width", x, l.max_stroke_width, true)? } else { check_num(l, "number", x)? }
        }
        Value::String(s) => {
            *chars += s.chars().count();
            if *chars > l.max_text_chars {
                return Err(Violation::TextTooLong { len: *chars, max: l.max_text_chars });
            }
        }
        Value::Array(a) => {
            too_many("elements", a.len(), l.max_pen_points)?;
            for x in a {
                walk(l, x, key, depth + 1, chars)?;
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                walk(l, x, k, depth + 1, chars)?;
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
    Ok(())
}

/// Validates one object: ids, bounds, text length, structural consistency (what the renderer indexes without checking).
pub fn check_object(l: &Limits, o: &Object) -> Result<(), Violation> {
    check_id(l, &o.id)?;
    check_id(l, &o.layer)?;
    if let Some(g) = &o.group {
        check_id(l, g)?;
    }
    match &o.kind {
        Kind::Box(b) => {
            check_frame(l, &b.frame)?;
            check_len("font size", b.label.size, l.max_font_size, false)?;
        }
        Kind::Image(i) => {
            check_frame(l, &i.frame)?;
            check_id(l, &i.asset)?;
        }
        Kind::Pen(p) => {
            too_many("points", p.pts.len(), l.max_pen_points)?;
            for q in p.pts.iter() {
                check_pt(l, "point", q)?;
            }
            if p.pressures.as_ref().is_some_and(|v| v.len() != p.pts.len()) {
                return Err(Violation::Malformed("pressure count differs from point count"));
            }
        }
        Kind::Table(t) => {
            check_pt(l, "table origin", &t.origin)?;
            let (rows, cols) = (t.row_h.len(), t.col_w.len());
            if rows == 0 || cols == 0 {
                return Err(Violation::Malformed("table without rows or columns"));
            }
            too_many("table rows", rows, l.max_table_dim)?;
            too_many("table columns", cols, l.max_table_dim)?;
            if t.cells.len() != rows * cols {
                return Err(Violation::Malformed("table cell count differs from rows x columns"));
            }
            for x in t.col_w.iter().chain(t.row_h.iter()) {
                check_len("table size", *x, l.max_object_size, false)?;
            }
            check_len("font size", t.size, l.max_font_size, false)?;
        }
        Kind::Connector(_) => {}
    }
    let v = serde_json::to_value(o).map_err(|_| Violation::Malformed("object cannot be serialized"))?;
    walk(l, &v, "", 0, &mut 0)?;
    // JSON writes a non-finite number as `null`: an object that does not come back identical had one (or cannot be relayed)
    match serde_json::from_value::<Object>(v) {
        Ok(back) if back == *o => Ok(()),
        _ => Err(Violation::Malformed("non-finite number or object that does not survive encoding")),
    }
}

/// Validates the operations of one `Submit` (counts, ids, names and every object). The number of pages of the document is checked
/// separately with [`check_page_count`].
pub fn check_ops(l: &Limits, ops: &[WireOp]) -> Result<(), Violation> {
    too_many("operations", ops.len(), l.max_ops_per_message)?;
    let (mut objects, mut ids, mut order_ids) = (0usize, 0usize, 0usize);
    for op in ops {
        match op {
            WireOp::Put { page, object } => {
                check_id(l, page)?;
                objects += 1;
                too_many("objects", objects, l.max_objects_per_message)?;
                check_object(l, object)?;
            }
            WireOp::Delete { page, ids: v } => {
                check_id(l, page)?;
                ids += v.len();
                too_many("identifiers", ids, l.max_ids_per_message)?;
                v.iter().try_for_each(|id| check_id(l, id))?;
            }
            WireOp::Order { page, ids: v } => {
                check_id(l, page)?;
                order_ids += v.len();
                too_many("identifiers in the stacking order", order_ids, l.max_order_ids)?;
                v.iter().try_for_each(|id| check_id(l, id))?;
            }
            WireOp::SetLayers { page, layers } => {
                check_id(l, page)?;
                too_many("layers", layers.len(), l.max_layers)?;
                for layer in layers {
                    check_id(l, &layer.id)?;
                    check_name(l, &layer.name)?;
                }
            }
            WireOp::AddPage { page, .. } => {
                check_id(l, &page.id)?;
                check_name(l, &page.name)?;
                too_many("layers", page.layers.len(), l.max_layers)?;
                objects += page.objects.len();
                too_many("objects", objects, l.max_objects_per_message)?;
                page.objects.iter().try_for_each(|o| check_object(l, o))?;
            }
            WireOp::RenamePage { page, name } => {
                check_id(l, page)?;
                check_name(l, name)?;
            }
            WireOp::RemovePage { page } => check_id(l, page)?,
            WireOp::SetName { name } => check_name(l, name)?,
            WireOp::SetBackground { .. } => {}
        }
    }
    Ok(())
}

/// Number of pages of the document after the operations.
pub fn check_page_count(l: &Limits, pages: usize) -> Result<(), Violation> {
    too_many("pages", pages, l.max_pages)
}

/// Size of an image to upload.
pub fn check_image_size(l: &Limits, size: u64) -> Result<(), Violation> {
    if size > l.max_image_bytes { Err(Violation::ImageTooLarge { size, max: l.max_image_bytes }) } else { Ok(()) }
}

pub fn check_asset_chunk(l: &Limits, c: &AssetChunk) -> Result<(), Violation> {
    too_many("bytes in a chunk", c.data.len(), l.max_asset_chunk_bytes)?;
    check_image_size(l, c.offset.saturating_add(c.data.len() as u64))
}

/// Checks the content of a message coming from a guest (its size is checked first with [`check_message_size`]).
pub fn check_message(l: &Limits, m: &Message) -> Result<(), Violation> {
    match m {
        Message::Hello(h) => check_name(l, &h.name),
        Message::Submit(s) => check_ops(l, &s.ops),
        Message::Transient { object, state, .. } => {
            check_id(l, object)?;
            // a state is shown in place of the object of the same identifier: never of another one
            if state.id != *object {
                return Err(Violation::Malformed("transient state does not belong to its object"));
            }
            check_object(l, state)
        }
        Message::StrokePoints { stroke, points, .. } => {
            check_id(l, stroke)?;
            too_many("points", points.len(), l.max_pen_points)?;
            points.iter().try_for_each(|p| check_pt(l, "point", p))
        }
        Message::Presence(p) => {
            check_id(l, &p.page)?;
            too_many("identifiers", p.selection.len(), l.max_ids_per_message)?;
            p.selection.iter().try_for_each(|id| check_id(l, id))?;
            if let Some(c) = &p.cursor {
                check_pt(l, "cursor", c)?;
            }
            if let Some(t) = &p.tool {
                check_name(l, t)?;
            }
            if let Some(e) = &p.editing {
                check_id(l, e)?;
            }
            if let Some(r) = &p.viewport {
                check_pt(l, "viewport", &Pt::new(r.x, r.y))?;
                check_len("viewport", r.w, l.max_abs_value, true)?;
                check_len("viewport", r.h, l.max_abs_value, true)?;
            }
            Ok(())
        }
        Message::AssetChunk(c) => check_asset_chunk(l, c),
        // only the host sends these: the guest checks them before showing them
        Message::Agent(a) => {
            check_name(l, &a.name)?;
            check_name(l, &a.action)?;
            too_many("identifiers", a.targets.len(), l.max_ids_per_message)?;
            a.targets.iter().try_for_each(|id| check_id(l, id))?;
            a.at.as_ref().map_or(Ok(()), |p| check_pt(l, "agent position", p))
        }
        Message::Participants { list } => {
            too_many("participants", list.len(), MAX_PARTICIPANTS_LISTED)?;
            list.iter().try_for_each(|p| check_name(l, &p.name))
        }
        Message::Assets { list } => {
            too_many("images", list.len(), MAX_ANNOUNCED_ASSETS)?;
            check_manifest(l, list)
        }
        _ => Ok(()),
    }
}

/// Most images one `Assets` message announces.
pub const MAX_ANNOUNCED_ASSETS: usize = 1_000;

/// Longest list of participants a guest accepts.
pub const MAX_PARTICIPANTS_LISTED: usize = 1_000;

/// Validates pages (identifiers, names, layers and every object); returns the number of objects. Used by a guest for what the host
/// sends (snapshot or part of one).
pub fn check_pages(l: &Limits, pages: &[Page]) -> Result<usize, Violation> {
    too_many("pages", pages.len(), l.max_pages)?;
    let mut objects = 0usize;
    for p in pages {
        check_id(l, &p.id)?;
        check_name(l, &p.name)?;
        too_many("layers", p.layers.len(), l.max_layers)?;
        for layer in &p.layers {
            check_id(l, &layer.id)?;
            check_name(l, &layer.name)?;
        }
        objects += p.objects.len();
        p.objects.iter().try_for_each(|o| check_object(l, o))?;
    }
    Ok(objects)
}

/// Structure of a whole document received from the host: page count, object count, no identifier used twice on a page.
pub fn check_snapshot_shape(l: &Limits, pages: &[Page], max_objects: usize) -> Result<(), Violation> {
    too_many("pages", pages.len(), l.max_pages)?;
    let total: usize = pages.iter().map(|p| p.objects.len()).sum();
    too_many("objects", total, max_objects)?;
    let mut seen = std::collections::HashSet::new();
    for p in pages {
        seen.clear();
        if !p.objects.iter().all(|o| seen.insert(&o.id)) {
            return Err(Violation::Malformed("object identifier used twice on a page"));
        }
    }
    Ok(())
}

/// Validates the list of images of a snapshot.
pub fn check_manifest(l: &Limits, assets: &[AssetInfo]) -> Result<(), Violation> {
    too_many("images", assets.len(), 100_000)?;
    for a in assets {
        check_id(l, &a.id)?;
        if a.hash.len() > 128 || a.mime.len() > 128 {
            return Err(Violation::Malformed("image description too long"));
        }
    }
    Ok(())
}

/// Bytes of images uploaded by one guest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quota {
    used: u64,
}

impl Quota {
    pub fn used(&self) -> u64 {
        self.used
    }
    /// Accounts for an image of `size` bytes, or refuses it (nothing is charged on refusal).
    pub fn charge(&mut self, l: &Limits, size: u64) -> Result<(), Violation> {
        check_image_size(l, size)?;
        let total = self.used.saturating_add(size);
        if total > l.guest_asset_quota_bytes {
            return Err(Violation::QuotaExceeded { used: self.used, size, max: l.guest_asset_quota_bytes });
        }
        self.used = total;
        Ok(())
    }
}

/// Token bucket driven by an explicit clock (milliseconds), so that it is deterministic.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    /// Thousandths of a token.
    tokens: u64,
    capacity: u64,
    /// Thousandths of a token per millisecond (= tokens per second).
    refill: u64,
    last_ms: u64,
}

impl RateLimiter {
    pub fn new(per_sec: u32, burst: u32) -> Self {
        let capacity = u64::from(burst.max(1)) * 1000;
        Self { tokens: capacity, capacity, refill: u64::from(per_sec), last_ms: 0 }
    }
    /// Takes one token if available.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        self.allow_n(now_ms, 1)
    }
    /// Takes `n` tokens (e.g. bytes, for a limiter built with bytes per second) if available; nothing is taken otherwise.
    pub fn allow_n(&mut self, now_ms: u64, n: u64) -> bool {
        let dt = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        self.tokens = self.tokens.saturating_add(dt.saturating_mul(self.refill)).min(self.capacity);
        let cost = n.saturating_mul(1000);
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }
    pub fn check(&mut self, now_ms: u64, what: &'static str) -> Result<(), Violation> {
        if self.allow(now_ms) { Ok(()) } else { Err(Violation::RateExceeded { what }) }
    }
}
