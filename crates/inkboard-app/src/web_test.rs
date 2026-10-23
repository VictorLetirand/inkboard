//! Test hook of the browser end-to-end bench (`crates/inkboard-app/web/test/`), only built with the `web-test` feature: the page gets
//! `window.__inkboard_test` with
//! - `join(link, name)`: what the "Join" window does when its button is clicked (an error shown by the window is `join_error` of `state()`);
//! - `leave()`: leaves the session;
//! - `state()`: a JSON string describing the session and the board on screen (see [`status`]).
//!
//! Calls from the page are queued and executed by [`frame`] on the next frame of the application.

use crate::app::App;
use crate::session_guest::GuestState;
use eframe::egui;
use serde_json::json;
use std::cell::RefCell;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

enum Command {
    Join { link: String, name: String },
    Leave,
}

#[derive(Default)]
struct Hook {
    commands: Vec<Command>,
    status: String,
    ctx: Option<egui::Context>,
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
        }
    }
    let status = status(app).to_string();
    HOOK.with(|h| h.borrow_mut().status = status);
}

/// `state`: `none` (no session), `joining`, `live`, `reconnecting`, `failed` (with `issue`: class, headline, detail), `ended`;
/// `mirror`: the host's board is on screen; `objects`/`ids`: the objects of all pages of the board on screen.
fn status(app: &App) -> serde_json::Value {
    let join_error = app.join.window.as_ref().and_then(|w| w.error.clone());
    let Some(g) = app.guest.as_ref() else { return json!({ "state": "none", "join_error": join_error }) };
    let state = g.state();
    let (name, issue) = match &state {
        GuestState::Joining => ("joining", None),
        GuestState::Live => ("live", None),
        GuestState::Reconnecting => ("reconnecting", None),
        GuestState::Failed(i) => ("failed", Some(json!({ "class": format!("{:?}", i.class), "headline": i.headline(), "detail": i.detail }))),
        GuestState::Ended(r) => ("ended", Some(json!({ "reason": format!("{r:?}") }))),
    };
    let mirror = app.guest_active();
    let mut ids: Vec<String> = if mirror { app.ed.doc.pages.iter().flat_map(|p| p.objects.iter().map(|o| o.id.to_string())).collect() } else { Vec::new() };
    ids.sort();
    json!({
        "state": name,
        "issue": issue,
        "join_error": join_error,
        "mirror": mirror,
        "mirror_ready": g.mirror_ready(),
        "host_name": g.host_name(),
        "participants": g.roster().len(),
        "objects": ids.len(),
        "ids": ids,
    })
}
