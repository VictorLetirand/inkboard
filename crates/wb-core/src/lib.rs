//! `wb-core` : moteur du tableau blanc, sans dépendance à une interface.
//!
//! - [`model`] : documents, pages, calques, objets.
//! - [`connector`] : flèches liées aux objets, recalculées à la demande.
//! - [`hit`] : boîtes englobantes et sélection.
//! - [`history`] / [`editor`] : modifications annulables.
//! - [`svg`] : export SVG ; [`layout`] : mise en page automatique de graphes.

pub mod connector;
pub mod editor;
pub mod geom;
pub mod history;
pub mod hit;
pub mod latex;
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
