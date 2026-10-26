//! Test hook of the browser end-to-end bench (`crates/inkboard-app/web/test/`), only built with the `web-test` feature: the page gets
//! `window.__inkboard_test` with
//! - `join(link, name)`: what the "Join" window does when its button is clicked (an error shown by the window is `join_error` of `state()`);
//! - `leave()`: leaves the session;
//! - `copy_image()`: "Copy as image" of the selection (or of the board);
//! - `set_timeouts(connect_ms, probe_ms, probe_delay_ms)`: shorter network time-outs for the sessions joined afterwards (silent-block scenarios);
//! - `state()`: a JSON string describing the session and the board on screen (see [`status`]).
//!
//! The session windows tell where their widgets are (`marks` of `state()`, in CSS pixels): the bench clicks them with the mouse or the
//! finger like a person would.
//!
//! Calls from the page are queued and executed by [`frame`] on the next frame of the application.

use crate::app::App;
use crate::session_guest::GuestState;
use eframe::egui;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

enum Command {
    Join { link: String, name: String },
    Leave,
    CopyImage,
}

#[derive(Default)]
struct Hook {
    commands: Vec<Command>,
    status: String,
    ctx: Option<egui::Context>,
    /// Where the widgets drawn by the last frame are: name -> [x, y, width, height].
    marks: BTreeMap<String, [f32; 4]>,
}

thread_local! {
    static HOOK: RefCell<Hook> = RefCell::new(Hook::default());
}

fn push(command: Command) {
    HOOK.with(|h| {
        let mut h = h.borrow_mut();
        h.commands.push(command);
        if let Some(ctx) = &h.ctx {
            ctx.request_repaint();
        }
    });
}

/// A widget of the interface says where it is (called while it is drawn).
pub fn mark(name: &str, rect: egui::Rect) {
    HOOK.with(|h| {
        h.borrow_mut().marks.insert(name.to_owned(), [rect.min.x, rect.min.y, rect.width(), rect.height()]);
    });
}

/// Puts `window.__inkboard_test` in the page.
pub fn install(ctx: &egui::Context) {
    HOOK.with(|h| {
        let mut h = h.borrow_mut();
        h.ctx = Some(ctx.clone());
        h.status = json!({ "state": "none" }).to_string();
    });
    let Some(window) = web_sys::window() else { return };
    let object = js_sys::Object::new();
    let set = |name: &str, f: &JsValue| {
        let _ = js_sys::Reflect::set(&object, &JsValue::from_str(name), f);
    };
    let join = Closure::<dyn Fn(String, String)>::new(|link, name| push(Command::Join { link, name }));
    set("join", join.as_ref().unchecked_ref());
    join.forget();
    let leave = Closure::<dyn Fn()>::new(|| push(Command::Leave));
    set("leave", leave.as_ref().unchecked_ref());
    leave.forget();
    let copy_image = Closure::<dyn Fn()>::new(|| push(Command::CopyImage));
    set("copy_image", copy_image.as_ref().unchecked_ref());
    copy_image.forget();
    let set_timeouts = Closure::<dyn Fn(f64, f64, f64)>::new(|connect_ms, probe_ms, probe_delay_ms| {
        crate::session_net::set_timeouts(connect_ms as u64, probe_ms as u64, probe_delay_ms as u64);
    });
    set("set_timeouts", set_timeouts.as_ref().unchecked_ref());
    set_timeouts.forget();
    let state = Closure::<dyn Fn() -> String>::new(|| HOOK.with(|h| h.borrow().status.clone()));
    set("state", state.as_ref().unchecked_ref());
    state.forget();
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__inkboard_test"), &object);
}

/// Runs the queued calls and refreshes the status; call it on every frame of the application.
pub fn frame(app: &mut App) {
    let commands = HOOK.with(|h| std::mem::take(&mut h.borrow_mut().commands));
    for command in commands {
        match command {
            Command::Join { link, name } => {
                app.open_join_window(Some(link));
                if let Some(w) = app.join.window.as_mut() {
                    w.name = name;
                }
                app.submit_join();
            }
            Command::Leave => app.leave_guest(),
            Command::CopyImage => app.copy_as_image(),
        }
    }
    let (marks, screen) = HOOK.with(|h| {
        let mut h = h.borrow_mut();
        (std::mem::take(&mut h.marks), h.ctx.as_ref().map(|c| c.content_rect().size()))
    });
    let mut status = status(app);
    status["marks"] = json!(marks);
    status["screen"] = json!(screen.map(|s| [s.x, s.y]));
    HOOK.with(|h| h.borrow_mut().status = status.to_string());
}

/// `state`: `none` (no session), `joining`, `live`, `reconnecting`, `failed` (with `issue`: class, headline, detail), `ended`;
/// `mirror`: the host's board is on screen; `objects`/`ids`: the objects of all pages of the board on screen.
fn status(app: &App) -> serde_json::Value {
    let join_error = app.join.window.as_ref().and_then(|w| w.error.clone());
    // the "Join" window: whether it is open, and whether its link came from the address of the page (typed otherwise)
    let window = app.join.window.as_ref().map(|w| json!({ "from_system": w.from_system, "name": w.name, "unknown_relay": w.unknown_relay }));
    let Some(g) = app.guest.as_ref() else { return json!({ "state": "none", "join_error": join_error, "window": window }) };
    let state = g.state();
    let (name, issue) = match &state {
        GuestState::Joining => ("joining", None),
        GuestState::Live => ("live", None),
        GuestState::Reconnecting => ("reconnecting", None),
        GuestState::Failed(i) => (
            "failed",
            Some(
                json!({ "class": format!("{:?}", i.class), "headline": i.headline(), "tips": i.tips(), "detail": i.detail, "diagnostic": i.diagnostic_text() }),
            ),
        ),
        GuestState::Ended(r) => ("ended", Some(json!({ "reason": format!("{r:?}"), "message": crate::join_ui::end_message(r) }))),
    };
    let mirror = app.guest_active();
    let mut ids: Vec<String> = if mirror { app.ed.doc.pages.iter().flat_map(|p| p.objects.iter().map(|o| o.id.to_string())).collect() } else { Vec::new() };
    ids.sort();
    let now = app.agent.remote.now();
    // the other participants as drawn: name, cursor (world units) and whether it is still visible
    let people: Vec<_> = app
        .agent
        .remote
        .people
        .iter()
        .map(|(id, p)| json!({ "id": id, "name": p.state.participant.name, "cursor": p.state.position_at(now).map(|c| [c.x, c.y]), "visible": p.state.opacity(now) > 0.0 }))
        .collect();
    let agent_now = crate::platform::Instant::now();
    let images = app.ed.doc.pages.iter().flat_map(|p| p.objects.iter()).filter(|o| matches!(o.kind, inkboard_core::model::Kind::Image(_))).count();
    // the objects that carry a field added by protocol v4 (drop shadow, arrowhead size, recolored SVG), with their frame in world units
    let fields: Vec<_> = app
        .ed
        .doc
        .pages
        .iter()
        .flat_map(|p| p.objects.iter().map(move |o| (p, o)))
        .filter_map(|(p, o)| {
            let recolor = match &o.kind {
                inkboard_core::model::Kind::Image(i) => i.recolor.len(),
                _ => 0,
            };
            let head_scale = match &o.kind {
                inkboard_core::model::Kind::Connector(c) => Some(c.head_scale),
                _ => None,
            };
            (o.style.shadow || recolor > 0 || head_scale.is_some_and(|s| (s - 1.0).abs() > f32::EPSILON)).then(|| {
                let f = p.bounds(o);
                json!({ "id": o.id.to_string(), "shadow": o.style.shadow, "head_scale": head_scale, "recolor": recolor, "frame": [f.x, f.y, f.w, f.h] })
            })
        })
        .collect();
    let view = app.ed.page().view;
    json!({
        "fields": fields,
        "view": [view.x, view.y, view.zoom],
        "canvas": [app.canvas_rect.min.x, app.canvas_rect.min.y],
        "state": name,
        "issue": issue,
        "join_error": join_error,
        "window": window,
        "role": format!("{:?}", g.role()),
        "pending": g.pending_len(),
        "attempt": g.reconnect_attempt(),
        "people": people,
        "agent": { "name": app.agent.state.participant.name, "cursor": app.agent.state.position_at(agent_now).map(|c| [c.x, c.y]), "visible": app.agent.state.opacity(agent_now) > 0.0 },
        "images": images,
        "assets": app.ed.doc.assets.len(),
        "read_only": app.view_only(),
        "mirror": mirror,
        "mirror_ready": g.mirror_ready(),
        "host_name": g.host_name(),
        "participants": g.roster().len(),
        "objects": ids.len(),
        "ids": ids,
    })
}
