//! `inkboard-core`: the whiteboard engine, with no UI dependency.
//!
//! - [`model`]: documents, pages, layers, objects.
//! - [`connector`]: arrows bound to objects, recomputed on demand.
//! - [`hit`]: bounding boxes and selection.
//! - [`history`] / [`editor`]: undoable modifications.
//! - [`svg`]: SVG export; [`layout`]: automatic graph layout.
//! - [`num`]: lossy numeric conversions with a documented behaviour.
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![cfg_attr(
    test,
    allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        clippy::float_cmp,
        clippy::many_single_char_names,
        clippy::similar_names,
        clippy::assert_is_empty,
        clippy::default_trait_access,
        clippy::format_collect,
        clippy::format_push_string,
        clippy::too_many_lines,
        clippy::items_after_statements,
        clippy::needless_raw_string_hashes,
        clippy::needless_pass_by_value,
        clippy::explicit_iter_loop,
        clippy::needless_for_each,
        clippy::collapsible_else_if,
        clippy::single_match_else,
        clippy::manual_assert,
        clippy::redundant_closure_for_method_calls,
        clippy::unnested_or_patterns,
        clippy::range_plus_one,
        clippy::clone_on_copy,
        clippy::match_same_arms,
        clippy::struct_excessive_bools
    )
)]

pub mod connector;
pub mod editor;
pub mod geom;
pub mod highlight;
pub mod history;
pub mod hit;
pub mod layout;
pub mod model;
pub mod num;
pub mod pen;
pub mod route;
pub mod serial;
pub mod spatial;
pub mod style;
pub mod svg;
pub mod svgcolor;
pub mod text;
pub mod transform;

pub use connector::{Anchor, Arrowhead, Connector, End, HeadExt, HeadPart, Route, head_glyph};
pub use editor::Editor;
pub use geom::{Frame, Pt, Rect};
pub use model::{
    Asset, AssetId, Background, BgPreset, BoxGeom, BoxObj, Document, Id, ImageObj, Kind, Label, Layer, Object, Page, Pattern, PenObj, Table, View,
};
pub use style::{Align, Color, Dash, FontFamily, Style};
pub use text::{ApproxMeasurer, Format, Measurer, RichText, Span, SpanStyle};
