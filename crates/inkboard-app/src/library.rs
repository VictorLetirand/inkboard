//! Icon library palette: a floating card with search, group / set filters and a virtualized grid of thumbnails. A click inserts the icon
//! (an SVG image object) at the center of the view.

use crate::app::App;
use crate::i18n::{t, tr};
use crate::platform::Pack;
use crate::theme;
use eframe::egui::{self, Align2, Color32, TextureHandle, TextureOptions, vec2};
use inkboard_core::num;
use inkboard_icons::{IconRef, Library};
use std::collections::HashMap;
use std::sync::Arc;

/// The pack and its once-opened `Library` (resolves the built-in icons of an imported draw.io file). The desktop embeds the pack once,
/// by `inkboard-icons` (feature `embedded`, shared with the MCP server); the web build downloads it when the library is first opened.
pub use crate::platform::shared_icons as shared;

/// `inkboard_formats::import_any`, with the icon library to resolve the built-in icons of draw.io files.
pub fn import(name: &str, content: &str, m: &dyn inkboard_core::text::Measurer) -> Result<inkboard_formats::Imported, inkboard_formats::FormatError> {
    match shared() {
        Some(lib) => inkboard_formats::import_any_with_icons(name, content, m, lib),
        None => inkboard_formats::import_any(name, content, m),
    }
}

const WIDTH: f32 = 372.0;
const CELL: f32 = 62.0;
/// Thumbnails rasterized per frame (the others wait for the next frame).
const BUDGET: usize = 24;
/// Thumbnail textures kept before the cache is emptied.
const MAX_TEXTURES: usize = 1200;
/// Results shown for a search.
const MAX_RESULTS: usize = 1500;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    lib: Option<Result<Arc<Library>, String>>,
    pub query: String,
    pub group: Option<String>,
    pub set: Option<usize>,
    focus: bool,
    textures: HashMap<IconRef, TextureHandle>,
    list_key: Option<(String, Option<String>, Option<usize>)>,
    list: Vec<IconRef>,
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette").field("open", &self.open).finish_non_exhaustive()
    }
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus = self.open;
    }

    /// The library, or `None` while the pack is still downloading (web).
    fn library(&mut self) -> Result<Option<Arc<Library>>, String> {
        if self.lib.is_none() {
            match crate::platform::request_icon_pack() {
                Pack::Ready(bytes) => self.lib = Some(Library::open(bytes).map(Arc::new)),
                Pack::Loading => return Ok(None),
                Pack::Failed(e) => return Err(e),
            }
        }
        self.lib.clone().expect("set above").map(Some)
    }

    /// Icons to show for the current filters (`None`: nothing chosen yet, show the groups).
    fn refresh_list(&mut self, lib: &Library) -> bool {
        let key = (self.query.trim().to_owned(), self.group.clone(), self.set);
        if self.list_key.as_ref() == Some(&key) {
            return !self.list.is_empty() || !key.0.is_empty() || key.1.is_some() || key.2.is_some();
        }
        let in_scope = |si: usize| self.set.is_none_or(|s| s == si) && self.group.as_ref().is_none_or(|g| &lib.sets[si].group == g);
        self.list = if !key.0.is_empty() {
            lib.search(&key.0, usize::MAX).into_iter().filter(|r| in_scope(r.set)).take(MAX_RESULTS).collect()
        } else if self.group.is_some() || self.set.is_some() {
            (0..lib.sets.len()).filter(|si| in_scope(*si)).flat_map(|si| (0..lib.sets[si].names.len()).map(move |ii| IconRef { set: si, icon: ii })).collect()
        } else {
            Vec::new()
        };
        self.list_key = Some(key);
        !self.list.is_empty() || !self.query.trim().is_empty() || self.group.is_some() || self.set.is_some()
    }

    /// Thumbnail texture of an icon (rasterized on first use, within the per-frame budget).
    fn thumb(&mut self, ctx: &egui::Context, lib: &Library, r: IconRef, budget: &mut usize) -> Option<TextureHandle> {
        if let Some(t) = self.textures.get(&r) {
            return Some(t.clone());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        if self.textures.len() >= MAX_TEXTURES {
            self.textures.clear();
        }
        let svg = lib.svg(r)?;
        let (w, h) = inkboard_icons::svg_dimensions(&svg).unwrap_or((24.0, 24.0));
        let px = (CELL - 12.0) * ctx.pixels_per_point();
        let scale = num::f32(f64::from(px) / w.max(h));
        let raster = inkboard_formats::raster::svg_to_raster(&svg, scale.clamp(0.01, 8.0), false).ok()?;
        let img = egui::ColorImage::from_rgba_premultiplied([raster.width as usize, raster.height as usize], &raster.rgba_premultiplied);
        let tex = ctx.load_texture(format!("icon-{}-{}", r.set, r.icon), img, TextureOptions::LINEAR);
        self.textures.insert(r, tex.clone());
        Some(tex)
    }
}

/// Group and set pickers.
fn filters(ui: &mut egui::Ui, pal_state: &mut Palette, lib: &Library) {
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 6.0) / 2.0;
        let all_groups = t("All groups");
        let before = pal_state.group.clone();
        egui::ComboBox::from_id_salt("icon-group").width(w).selected_text(pal_state.group.clone().unwrap_or_else(|| all_groups.clone())).show_ui(ui, |ui| {
            if ui.selectable_label(pal_state.group.is_none(), &all_groups).clicked() {
                pal_state.group = None;
            }
            for g in lib.groups() {
                if ui.selectable_label(pal_state.group.as_deref() == Some(g), g).clicked() {
                    pal_state.group = Some(g.to_owned());
                }
            }
        });
        if pal_state.group != before {
            pal_state.set = None;
        }
        let all_sets = t("All sets");
        let sets: Vec<usize> = (0..lib.sets.len()).filter(|si| pal_state.group.as_ref().is_none_or(|g| &lib.sets[*si].group == g)).collect();
        let label = pal_state.set.map_or_else(|| all_sets.clone(), |s| lib.sets[s].title.clone());
        egui::ComboBox::from_id_salt("icon-set").width(w).selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(pal_state.set.is_none(), &all_sets).clicked() {
                pal_state.set = None;
            }
            for si in sets {
                if ui.selectable_label(pal_state.set == Some(si), &lib.sets[si].title).clicked() {
                    pal_state.set = Some(si);
                }
            }
        });
    });
}

/// Grid of the icons found; returns the one clicked.
fn icon_grid(ui: &mut egui::Ui, palette: &mut Palette, lib: &Library, max_height: f32, pal: &theme::Palette) -> Option<IconRef> {
    let mut picked = None;
    let cols = num::to_usize(f64::from((ui.available_width() / CELL).floor())).max(1);
    let total = palette.list.len();
    let rows = total.div_ceil(cols);
    let mut budget = BUDGET;
    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(max_height).show_rows(ui, CELL, rows, |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for row in range {
            ui.horizontal(|ui| {
                for c in 0..cols {
                    let Some(r) = palette.list.get(row * cols + c).copied() else { break };
                    let (rect, resp) = ui.allocate_exact_size(vec2(CELL, CELL), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect.shrink(2.0), 8.0, pal.ui2);
                    }
                    if let Some(tex) = palette.thumb(ui.ctx(), lib, r, &mut budget) {
                        let size = tex.size_vec2() / ui.ctx().pixels_per_point();
                        let tint = if lib.sets[r.set].mono { pal.tx } else { Color32::WHITE };
                        let target = egui::Rect::from_center_size(rect.center(), size.min(vec2(CELL - 12.0, CELL - 12.0)));
                        ui.painter().image(tex.id(), target, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
                    }
                    let resp = resp.on_hover_text(format!("{}\n{} · {}", lib.name(r).unwrap_or(""), lib.sets[r.set].group, lib.sets[r.set].title));
                    if resp.clicked() {
                        picked = Some(r);
                    }
                }
            });
        }
    });
    picked
}

/// Draws the palette card when it is open.
pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.palette.open {
        return;
    }
    let lib = match app.palette.library() {
        Ok(Some(l)) => l,
        Ok(None) => {
            theme::floating(ctx, "icon-palette", Align2::RIGHT_TOP, vec2(-12.0, 64.0), 10, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t("Loading the icon library…"));
                });
            });
            return;
        }
        Err(e) => {
            theme::floating(ctx, "icon-palette", Align2::RIGHT_TOP, vec2(-12.0, 64.0), 10, |ui| {
                ui.label(tr!("Icon library unavailable: {}", e));
            });
            return;
        }
    };
    let pal = theme::current(ctx);
    let height = (ctx.content_rect().height() - 64.0 - 76.0).max(260.0);
    let mut insert: Option<IconRef> = None;
    let mut close = false;
    theme::floating(ctx, "icon-palette", Align2::RIGHT_TOP, vec2(-12.0, 64.0), 10, |ui| {
        ui.set_width(WIDTH - 20.0);
        ui.set_height(height - 20.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("Icon library")).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "close", false, true).on_hover_text(t("Close")).clicked() {
                    close = true;
                }
                ui.label(egui::RichText::new(tr!("{} icons", lib.icon_count())).size(11.5).color(pal.mute)).on_hover_text(format!(
                    "{}
{}",
                    t("Built from:"),
                    lib.sources_summary()
                ));
            });
        });
        ui.add_space(4.0);
        let search =
            egui::TextEdit::singleline(&mut app.palette.query).hint_text(t("Search icons (e.g. lambda, router, github)…")).desired_width(f32::INFINITY);
        let resp = ui.add(search);
        if app.palette.focus {
            resp.request_focus();
            app.palette.focus = false;
        }
        ui.add_space(4.0);
        filters(ui, &mut app.palette, &lib);
        ui.add_space(6.0);
        let has_list = app.palette.refresh_list(&lib);
        if !has_list {
            // landing: the groups with their icon counts
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(height - 150.0).show(ui, |ui| {
                for g in lib.groups() {
                    let n: usize = lib.sets.iter().filter(|s| s.group == g).map(|s| s.names.len()).sum();
                    let sets = lib.sets.iter().filter(|s| s.group == g).count();
                    if ui.add_sized([ui.available_width(), 30.0], egui::Button::new(format!("{g}   ·   {}", tr!("{} icons in {} sets", n, sets)))).clicked() {
                        app.palette.group = Some(g.to_owned());
                        app.palette.set = None;
                    }
                }
            });
        } else if app.palette.list.is_empty() {
            ui.label(egui::RichText::new(t("No icon matches.")).color(pal.mute));
        } else {
            insert = icon_grid(ui, &mut app.palette, &lib, height - 150.0, &pal);
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t("Logos and product names belong to their respective owners; they are provided to label diagrams, as in draw.io."))
                .size(10.5)
                .color(pal.mute),
        );
    });
    if close {
        app.palette.open = false;
    }
    if let Some(r) = insert {
        app.insert_icon(&lib, r);
    }
}
