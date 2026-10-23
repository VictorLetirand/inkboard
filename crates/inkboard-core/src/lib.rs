//! `inkboard-core`: the whiteboard engine, with no UI dependency.
//!
//! - [`model`]: documents, pages, layers, objects.
//! - [`connector`]: arrows bound to objects, recomputed on demand.
//! - [`hit`]: bounding boxes and selection.
//! - [`history`] / [`editor`]: undoable modifications.
//! - [`svg`]: SVG export; [`layout`]: automatic graph layout.

pub mod connector;
pub mod editor;
pub mod geom;
pub mod highlight;
pub mod history;
pub mod hit;
pub mod layout;
pub mod model;
pub mod pen;
pub mod route;
pub mod serial;
pub mod spatial;
pub mod style;
pub mod svg;
pub mod text;
pub mod transform;

pub use connector::{Anchor, Arrowhead, Connector, End, Route};
pub use editor::Editor;
pub use geom::{Frame, Pt, Rect};
pub use model::{
    Asset, AssetId, Background, BgPreset, BoxGeom, BoxObj, Document, Id, ImageObj, Kind, Label, Layer, Object, Page, Pattern, PenObj, Table, View,
};
pub use style::{Align, Color, Dash, FontFamily, Style};
pub use text::{ApproxMeasurer, Format, Measurer, RichText, Span, SpanStyle};
