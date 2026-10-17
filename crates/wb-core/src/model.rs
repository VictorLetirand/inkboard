//! Modèle de document : objets, pages, calques, ressources.

use crate::connector::Connector;
use crate::geom::{Frame, Pt, Rect};
use crate::style::{Align, FontFamily, Style};
use crate::text::{Measurer, RichText, TextLayout, layout_rich};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/* ------------------------------------------------------------------ Id */

/// Identifiant opaque d'objet, de page, de calque ou de ressource.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);
static ID_SEED: AtomicU64 = AtomicU64::new(0);

/// Initialise la graine des identifiants (utile sur les plateformes sans horloge).
pub fn seed_ids(seed: u64) {
    ID_SEED.store(seed, Ordering::Relaxed);
}

fn next_seed() -> u64 {
    let s = ID_SEED.load(Ordering::Relaxed);
    if s != 0 {
        return s;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9, |d| d.as_nanos() as u64);
    #[cfg(target_arch = "wasm32")]
    let t = 0x9E37_79B9_7F4A_7C15u64;
    let t = t | 1;
    ID_SEED.store(t, Ordering::Relaxed);
    t
}

impl Id {
    /// Nouvel identifiant unique (base 36, 10 caractères).
    pub fn new() -> Id {
        let mut x = next_seed() ^ ID_COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        x ^= x >> 33;
        let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let s: String = (0..10)
            .map(|_| {
                let d = digits[(x % 36) as usize] as char;
                x /= 36;
                d
            })
            .collect();
        Id(s)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for Id {
    fn default() -> Self {
        Id::new()
    }
}
impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id(s.to_owned())
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type AssetId = Id;

/* --------------------------------------------------------------- Texte */

/// Texte d'un objet et sa mise en forme générale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    #[serde(default)]
    pub text: RichText,
    #[serde(default)]
    pub family: FontFamily,
    pub size: f64,
    #[serde(default)]
    pub align: Align,
}

impl Default for Label {
    fn default() -> Self {
        Self { text: RichText::default(), family: FontFamily::Sans, size: 20.0, align: Align::Center }
    }
}

impl Label {
    pub fn new(text: &str, size: f64, align: Align) -> Self {
        Self { text: RichText::plain(text), family: FontFamily::Sans, size, align }
    }
    pub fn line_height(&self) -> f64 {
        self.size * 1.3
    }
}

/* ------------------------------------------------------------- Formes */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxGeom {
    Rect,
    Ellipse,
    Diamond,
    Triangle,
    /// Note adhésive (fond coloré, ombre).
    Sticky,
    /// Texte libre (sans contour ni fond, dimension automatique).
    Text,
}

impl BoxGeom {
    /// Forme « contour » utilisée pour les points d'accroche et les tests de clic.
    pub fn outline(self) -> Outline {
        match self {
            BoxGeom::Ellipse => Outline::Ellipse,
            BoxGeom::Diamond => Outline::Diamond,
            BoxGeom::Triangle => Outline::Triangle,
            _ => Outline::Rect,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    Rect,
    Ellipse,
    Diamond,
    Triangle,
}

impl Outline {
    /// Polygone du contour dans le repère local (origine au centre).
    pub fn polygon(self, w: f64, h: f64) -> Vec<Pt> {
        let (hw, hh) = (w / 2.0, h / 2.0);
        match self {
            Outline::Diamond => vec![Pt::new(0.0, -hh), Pt::new(hw, 0.0), Pt::new(0.0, hh), Pt::new(-hw, 0.0)],
            Outline::Triangle => vec![Pt::new(0.0, -hh), Pt::new(hw, hh), Pt::new(-hw, hh)],
            _ => vec![Pt::new(-hw, -hh), Pt::new(hw, -hh), Pt::new(hw, hh), Pt::new(-hw, hh)],
        }
    }
}

/// Objet « boîte » : forme, note adhésive ou texte libre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxObj {
    pub geom: BoxGeom,
    pub frame: Frame,
    #[serde(default)]
    pub label: Label,
}

impl BoxObj {
    /// Zone (repère local) dans laquelle le texte est disposé.
    pub fn label_rect(&self) -> Rect {
        let (w, h) = (self.frame.w, self.frame.h);
        let (hw, hh) = (w / 2.0, h / 2.0);
        match self.geom {
            BoxGeom::Text => Rect::new(-hw + 3.0, -hh, (w - 6.0).max(1.0), h),
            BoxGeom::Sticky => Rect::new(-hw + 14.0, -hh + 14.0, (w - 28.0).max(1.0), (h - 28.0).max(1.0)),
            BoxGeom::Ellipse => Rect::new(-hw * 0.7, -hh * 0.7, w * 0.7, h * 0.7),
            BoxGeom::Diamond => Rect::new(-hw * 0.5, -hh * 0.5, w * 0.5, h * 0.5),
            BoxGeom::Triangle => Rect::new(-hw * 0.5, -hh * 0.1, w * 0.5, h * 0.5),
            BoxGeom::Rect => Rect::new(-hw + 8.0, -hh + 8.0, (w - 16.0).max(1.0), (h - 16.0).max(1.0)),
        }
    }

    /// Mise en page du texte dans [`BoxObj::label_rect`].
    pub fn layout(&self, m: &dyn Measurer) -> TextLayout {
        let wrap = (self.geom != BoxGeom::Text).then(|| self.label_rect().w);
        layout_rich(&self.label.text, wrap, m, self.label.family, self.label.size)
    }

    /// Adapte la taille d'un texte libre à son contenu ; agrandit les autres formes si le texte déborde.
    pub fn fit(&mut self, m: &dyn Measurer) {
        let lay = self.layout(m);
        if self.geom == BoxGeom::Text {
            self.frame.w = (lay.width.ceil() + 6.0).max(12.0);
            self.frame.h = lay.height.max(self.label.line_height());
        } else if !self.label.text.is_empty() {
            let avail = self.label_rect().h;
            if lay.height > avail {
                let ratio = match self.geom {
                    BoxGeom::Ellipse => 0.7,
                    BoxGeom::Diamond | BoxGeom::Triangle => 0.5,
                    _ => 1.0,
                };
                self.frame.h += (lay.height - avail) / ratio;
            }
        }
    }
}

/// Image posée sur le tableau.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageObj {
    pub frame: Frame,
    pub asset: AssetId,
}

/// Trait à main levée (points en coordonnées monde).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenObj {
    pub pts: Arc<Vec<Pt>>,
    #[serde(default)]
    pub highlighter: bool,
}

/// Tableau de cellules (lignes × colonnes), chaque cellule contenant du texte riche.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub origin: Pt,
    pub col_w: Vec<f64>,
    pub row_h: Vec<f64>,
    /// Cellules en ordre ligne par ligne.
    pub cells: Vec<RichText>,
    #[serde(default)]
    pub header: bool,
    #[serde(default)]
    pub family: FontFamily,
    pub size: f64,
}

impl Table {
    pub fn new(origin: Pt, rows: usize, cols: usize) -> Self {
        let (rows, cols) = (rows.max(1), cols.max(1));
        Self {
            origin,
            col_w: vec![140.0; cols],
            row_h: vec![44.0; rows],
            cells: vec![RichText::default(); rows * cols],
            header: true,
            family: FontFamily::Sans,
            size: 18.0,
        }
    }
    pub fn rows(&self) -> usize {
        self.row_h.len()
    }
    pub fn cols(&self) -> usize {
        self.col_w.len()
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.origin.x, self.origin.y, self.col_w.iter().sum(), self.row_h.iter().sum())
    }
    pub fn cell(&self, r: usize, c: usize) -> Option<&RichText> {
        (r < self.rows() && c < self.cols()).then(|| &self.cells[r * self.cols() + c])
    }
    pub fn cell_mut(&mut self, r: usize, c: usize) -> Option<&mut RichText> {
        let cols = self.cols();
        (r < self.rows() && c < cols).then(move || &mut self.cells[r * cols + c])
    }
    pub fn cell_rect(&self, r: usize, c: usize) -> Rect {
        let x = self.origin.x + self.col_w[..c].iter().sum::<f64>();
        let y = self.origin.y + self.row_h[..r].iter().sum::<f64>();
        Rect::new(x, y, self.col_w[c], self.row_h[r])
    }
    /// Cellule située sous un point monde.
    pub fn cell_at(&self, p: Pt) -> Option<(usize, usize)> {
        if !self.rect().contains(p) {
            return None;
        }
        let (mut x, mut y) = (self.origin.x, self.origin.y);
        let c = self.col_w.iter().position(|w| {
            x += w;
            p.x <= x
        })?;
        let r = self.row_h.iter().position(|h| {
            y += h;
            p.y <= y
        })?;
        Some((r, c))
    }
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows());
        let cols = self.cols();
        self.row_h.insert(at, 44.0);
        for i in 0..cols {
            self.cells.insert(at * cols + i, RichText::default());
        }
    }
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.cols());
        let old_cols = self.cols();
        self.col_w.insert(at, 140.0);
        for r in (0..self.rows()).rev() {
            self.cells.insert(r * old_cols + at, RichText::default());
        }
    }
    pub fn remove_row(&mut self, at: usize) {
        if self.rows() > 1 && at < self.rows() {
            let cols = self.cols();
            self.cells.drain(at * cols..(at + 1) * cols);
            self.row_h.remove(at);
        }
    }
    pub fn remove_col(&mut self, at: usize) {
        if self.cols() > 1 && at < self.cols() {
            let cols = self.cols();
            for r in (0..self.rows()).rev() {
                self.cells.remove(r * cols + at);
            }
            self.col_w.remove(at);
        }
    }
}

/* ------------------------------------------------------------- Objets */

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    Box(BoxObj),
    Pen(PenObj),
    Connector(Connector),
    Image(ImageObj),
    Table(Table),
}

fn default_layer() -> Id {
    Id::from("main")
}
fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: Id,
    pub kind: Kind,
    #[serde(default)]
    pub style: Style,
    /// Groupe : les objets partageant un identifiant se sélectionnent ensemble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Id>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default = "default_layer")]
    pub layer: Id,
}

impl Object {
    pub fn new(kind: Kind, style: Style) -> Self {
        Self { id: Id::new(), kind, style, group: None, locked: false, layer: default_layer() }
    }

    /// Forme, note ou texte.
    pub fn new_box(geom: BoxGeom, frame: Frame, style: Style, label: Label) -> Self {
        Self::new(Kind::Box(BoxObj { geom, frame, label }), style)
    }

    pub fn new_pen(pts: Vec<Pt>, highlighter: bool, style: Style) -> Self {
        Self::new(Kind::Pen(PenObj { pts: Arc::new(pts), highlighter }), style)
    }

    /// Cadre pour les objets rectangulaires (forme, image, tableau).
    pub fn frame(&self) -> Option<Frame> {
        match &self.kind {
            Kind::Box(b) => Some(b.frame),
            Kind::Image(i) => Some(i.frame),
            Kind::Table(t) => Some(Frame::from_rect(t.rect())),
            _ => None,
        }
    }

    /// Les connecteurs peuvent s'accrocher à cet objet.
    pub fn is_attachable(&self) -> bool {
        self.frame().is_some()
    }

    /// Déplace l'objet. Les extrémités liées d'un connecteur ne bougent pas (elles suivent leur cible).
    pub fn translate(&mut self, d: Pt) {
        match &mut self.kind {
            Kind::Box(b) => {
                b.frame.x += d.x;
                b.frame.y += d.y;
            }
            Kind::Image(i) => {
                i.frame.x += d.x;
                i.frame.y += d.y;
            }
            Kind::Pen(p) => Arc::make_mut(&mut p.pts).iter_mut().for_each(|q| *q = *q + d),
            Kind::Connector(c) => c.translate_free(d),
            Kind::Table(t) => t.origin = t.origin + d,
        }
    }

    /// Rotation autour de `c`. Renvoie `false` si l'objet ne peut pas tourner (tableau).
    pub fn rotate_about(&mut self, c: Pt, a: f64) -> bool {
        match &mut self.kind {
            Kind::Box(BoxObj { frame, .. }) | Kind::Image(ImageObj { frame, .. }) => {
                let nc = frame.center().rotate(c, a);
                frame.x = nc.x - frame.w / 2.0;
                frame.y = nc.y - frame.h / 2.0;
                frame.angle += a;
                true
            }
            Kind::Pen(p) => {
                Arc::make_mut(&mut p.pts).iter_mut().for_each(|q| *q = q.rotate(c, a));
                true
            }
            Kind::Connector(cn) => {
                cn.map_free(|q| q.rotate(c, a));
                true
            }
            Kind::Table(_) => false,
        }
    }

    /// Mise à l'échelle par rapport à `o` (étirement non uniforme possible).
    pub fn scale_about(&mut self, o: Pt, sx: f64, sy: f64) {
        let map = |p: Pt| Pt::new(o.x + (p.x - o.x) * sx, o.y + (p.y - o.y) * sy);
        match &mut self.kind {
            Kind::Box(BoxObj { frame, .. }) | Kind::Image(ImageObj { frame, .. }) => {
                let nc = map(frame.center());
                frame.w = (frame.w * sx).abs();
                frame.h = (frame.h * sy).abs();
                frame.x = nc.x - frame.w / 2.0;
                frame.y = nc.y - frame.h / 2.0;
            }
            Kind::Pen(p) => Arc::make_mut(&mut p.pts).iter_mut().for_each(|q| *q = map(*q)),
            Kind::Connector(c) => c.map_free(map),
            Kind::Table(t) => {
                t.origin = map(t.origin);
                t.col_w.iter_mut().for_each(|w| *w = (*w * sx).abs().max(20.0));
                t.row_h.iter_mut().for_each(|h| *h = (*h * sy).abs().max(16.0));
            }
        }
    }
}

/* -------------------------------------------------------------- Pages */

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
}

impl Layer {
    pub fn main() -> Self {
        Layer { id: default_layer(), name: "Calque 1".into(), visible: true, locked: false }
    }
}

/// Position et zoom de la vue sur une page.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}
impl Default for View {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, zoom: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: Id,
    pub name: String,
    pub layers: Vec<Layer>,
    /// Ordre de dessin : le dernier objet est au premier plan.
    pub objects: Vec<Object>,
    #[serde(default)]
    pub view: View,
}

impl Page {
    pub fn new(name: &str) -> Self {
        Self { id: Id::new(), name: name.to_owned(), layers: vec![Layer::main()], objects: Vec::new(), view: View::default() }
    }
    pub fn index_of(&self, id: &Id) -> Option<usize> {
        self.objects.iter().position(|o| &o.id == id)
    }
    pub fn get(&self, id: &Id) -> Option<&Object> {
        self.objects.iter().find(|o| &o.id == id)
    }
    pub fn layer(&self, id: &Id) -> Option<&Layer> {
        self.layers.iter().find(|l| &l.id == id)
    }
    pub fn is_visible(&self, o: &Object) -> bool {
        self.layer(&o.layer).is_none_or(|l| l.visible)
    }
    pub fn is_locked(&self, o: &Object) -> bool {
        o.locked || self.layer(&o.layer).is_some_and(|l| l.locked)
    }
    /// Identifiants des objets dont le groupe est touché par `ids`.
    pub fn expand_groups(&self, ids: &[Id]) -> Vec<Id> {
        let groups: Vec<&Id> = self.objects.iter().filter(|o| ids.contains(&o.id)).filter_map(|o| o.group.as_ref()).collect();
        self.objects.iter().filter(|o| ids.contains(&o.id) || o.group.as_ref().is_some_and(|g| groups.contains(&g))).map(|o| o.id.clone()).collect()
    }
}

/* ------------------------------------------------------------ Document */

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BgPreset {
    White,
    Cream,
    Gray,
    Slate,
    Black,
    Green,
}

impl BgPreset {
    pub const ALL: [BgPreset; 6] = [BgPreset::White, BgPreset::Cream, BgPreset::Gray, BgPreset::Slate, BgPreset::Black, BgPreset::Green];
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            BgPreset::White => (0xff, 0xff, 0xff),
            BgPreset::Cream => (0xfb, 0xf5, 0xe6),
            BgPreset::Gray => (0xe9, 0xec, 0xef),
            BgPreset::Slate => (0x26, 0x2b, 0x38),
            BgPreset::Black => (0x10, 0x11, 0x14),
            BgPreset::Green => (0x1e, 0x3a, 0x31),
        }
    }
    pub fn is_dark(self) -> bool {
        matches!(self, BgPreset::Slate | BgPreset::Black | BgPreset::Green)
    }
    pub fn hex(self) -> String {
        let (r, g, b) = self.rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }
    pub fn label(self) -> &'static str {
        match self {
            BgPreset::White => "Blanc",
            BgPreset::Cream => "Crème",
            BgPreset::Gray => "Gris",
            BgPreset::Slate => "Ardoise",
            BgPreset::Black => "Noir",
            BgPreset::Green => "Tableau vert",
        }
    }
    /// Préréglage le plus proche d'une couleur quelconque.
    pub fn nearest(rgb: (u8, u8, u8)) -> BgPreset {
        let d = |p: BgPreset| {
            let c = p.rgb();
            let f = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
            f(c.0, rgb.0) + f(c.1, rgb.1) + f(c.2, rgb.2)
        };
        *BgPreset::ALL.iter().min_by_key(|p| d(**p)).unwrap_or(&BgPreset::White)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    None,
    Dots,
    Grid,
    Lines,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    pub preset: BgPreset,
    pub pattern: Pattern,
}
impl Default for Background {
    fn default() -> Self {
        Self { preset: BgPreset::White, pattern: Pattern::Dots }
    }
}

/// Ressource binaire (image) stockée une seule fois dans le document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub mime: String,
    #[serde(with = "b64")]
    pub data: Arc<Vec<u8>>,
}

mod b64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;
    pub fn serialize<S: Serializer>(v: &Arc<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v.as_slice()))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<u8>>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s.as_bytes()).map(Arc::new).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    #[serde(default)]
    pub background: Background,
    pub pages: Vec<Page>,
    #[serde(default)]
    pub assets: BTreeMap<AssetId, Asset>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("Sans titre")
    }
}

impl Document {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned(), background: Background::default(), pages: vec![Page::new("Page 1")], assets: BTreeMap::new() }
    }
    pub fn page(&self, id: &Id) -> Option<&Page> {
        self.pages.iter().find(|p| &p.id == id)
    }
    pub fn add_asset(&mut self, mime: &str, data: Vec<u8>) -> AssetId {
        let id = Id::new();
        self.assets.insert(id.clone(), Asset { mime: mime.to_owned(), data: Arc::new(data) });
        id
    }
    /// Supprime les ressources que plus aucun objet ne référence.
    pub fn prune_assets(&mut self) {
        let used: std::collections::HashSet<&Id> =
            self.pages.iter().flat_map(|p| p.objects.iter()).filter_map(|o| if let Kind::Image(i) = &o.kind { Some(&i.asset) } else { None }).collect();
        let keep: Vec<Id> = self.assets.keys().filter(|k| used.contains(k)).cloned().collect();
        self.assets.retain(|k, _| keep.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::ApproxMeasurer;

    #[test]
    fn ids_are_unique() {
        let ids: std::collections::HashSet<Id> = (0..5000).map(|_| Id::new()).collect();
        assert_eq!(ids.len(), 5000);
    }

    #[test]
    fn text_box_fits_content() {
        let mut b = BoxObj { geom: BoxGeom::Text, frame: Frame::new(0.0, 0.0, 1.0, 1.0), label: Label::new("Bonjour\nle monde", 20.0, Align::Left) };
        b.fit(&ApproxMeasurer);
        assert!(b.frame.w > 40.0);
        assert!((b.frame.h - 2.0 * 26.0).abs() < 1e-9);
    }

    #[test]
    fn rect_grows_when_text_overflows() {
        let mut b = BoxObj {
            geom: BoxGeom::Rect,
            frame: Frame::new(0.0, 0.0, 100.0, 40.0),
            label: Label::new("un texte vraiment très long qui déborde", 20.0, Align::Center),
        };
        let before = b.frame.h;
        b.fit(&ApproxMeasurer);
        assert!(b.frame.h > before);
    }

    #[test]
    fn table_structure_edits() {
        let mut t = Table::new(Pt::ZERO, 2, 2);
        t.cell_mut(1, 1).unwrap().set_plain("x");
        t.insert_row(1);
        assert_eq!(t.rows(), 3);
        assert_eq!(t.cell(2, 1).unwrap().to_plain(), "x");
        t.insert_col(0);
        assert_eq!(t.cols(), 3);
        assert_eq!(t.cell(2, 2).unwrap().to_plain(), "x");
        t.remove_col(0);
        t.remove_row(1);
        assert_eq!((t.rows(), t.cols()), (2, 2));
        assert_eq!(t.cell(1, 1).unwrap().to_plain(), "x");
        assert_eq!(t.cell_at(Pt::new(150.0, 50.0)), Some((1, 1)));
        assert_eq!(t.cell_at(Pt::new(-5.0, 5.0)), None);
    }

    #[test]
    fn rotate_and_scale_box() {
        let mut o = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 100.0, 50.0), Style::default(), Label::default());
        o.rotate_about(Pt::new(50.0, 25.0), 0.5);
        let f = o.frame().unwrap();
        assert!((f.angle - 0.5).abs() < 1e-12);
        assert!(f.center().dist(Pt::new(50.0, 25.0)) < 1e-9);
        o.scale_about(Pt::new(50.0, 25.0), 2.0, 2.0);
        assert!((o.frame().unwrap().w - 200.0).abs() < 1e-9);
    }

    #[test]
    fn pen_is_copy_on_write() {
        let mut a = Object::new_pen(vec![Pt::ZERO, Pt::new(1.0, 1.0)], false, Style::default());
        let b = a.clone();
        a.translate(Pt::new(5.0, 5.0));
        let (Kind::Pen(pa), Kind::Pen(pb)) = (&a.kind, &b.kind) else { panic!() };
        assert_eq!(pa.pts[0], Pt::new(5.0, 5.0));
        assert_eq!(pb.pts[0], Pt::ZERO);
    }
}
