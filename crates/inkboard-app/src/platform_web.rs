//! Browser side of [`crate::platform`].
//!
//! What the browser delivers late (a picked file, a pasted image, the downloaded icon pack) is queued here, then taken by
//! [`poll`] / [`request_icon_pack`] on the next frame; the context given to [`init`] wakes the application up.

use super::{Incoming, Pack};
use crate::app::RawImage;
use eframe::egui;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::{JsFuture, spawn_local};

pub use web_time::Instant;

/// The browser has no data folder: settings and autosave are not kept between visits.
pub const PERSISTS: bool = false;
/// "Save" and "Export" hand the file to the browser as a download.
pub const DOWNLOADS: bool = true;

/// File of the icon pack, next to the page.
const PACK_URL: &str = "icons.bin";

enum PackState {
    Idle,
    Loading,
    Ready(&'static [u8]),
    Failed(String),
}

thread_local! {
    static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static INBOX: RefCell<Vec<Incoming>> = const { RefCell::new(Vec::new()) };
    static PACK: RefCell<PackState> = const { RefCell::new(PackState::Idle) };
    static ICONS: Cell<Option<&'static inkboard_icons::Library>> = const { Cell::new(None) };
    /// The browser said the network is back (or the tab is visible again) since the session last looked.
    static NET_EVENT: Cell<bool> = const { Cell::new(false) };
}

/// Called once at startup: remembers the context (to wake the application up), listens to the clipboard, to the address of the page
/// (an invitation link) and to the browser's network events.
pub fn init(ctx: &egui::Context) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    install_paste_listener();
    install_link_listener();
    install_network_listeners();
}

fn wake() {
    CTX.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}

fn push(ev: Incoming) {
    INBOX.with(|i| i.borrow_mut().push(ev));
    wake();
}

/// What arrived since the last frame.
pub fn poll() -> Vec<Incoming> {
    INBOX.with(|i| std::mem::take(&mut *i.borrow_mut()))
}

/// No folder to keep the settings in (see [`PERSISTS`]).
pub fn data_dir() -> PathBuf {
    PathBuf::from("/inkboard")
}

/* ------------------------------------------------------------------ icon pack */

/// The state of the icon pack; the first call starts the download.
pub fn request_icon_pack() -> Pack {
    PACK.with(|p| {
        let mut p = p.borrow_mut();
        if matches!(*p, PackState::Idle) {
            *p = PackState::Loading;
            spawn_local(async {
                let state = match fetch_bytes(PACK_URL).await {
                    Ok(bytes) => PackState::Ready(Box::leak(bytes.into_boxed_slice())),
                    Err(e) => PackState::Failed(e),
                };
                PACK.with(|p| *p.borrow_mut() = state);
                wake();
            });
        }
        match &*p {
            PackState::Ready(b) => Pack::Ready(b),
            PackState::Failed(e) => Pack::Failed(e.clone()),
            PackState::Idle | PackState::Loading => Pack::Loading,
        }
    })
}

/// The pack bytes once downloaded (never starts the download).
pub fn icon_pack() -> Option<&'static [u8]> {
    PACK.with(|p| if let PackState::Ready(b) = &*p.borrow() { Some(*b) } else { None })
}

/// The once-opened `Library` of the downloaded pack (`None` until then).
pub fn shared_icons() -> Option<&'static inkboard_icons::Library> {
    if ICONS.with(Cell::get).is_none() {
        let lib = inkboard_icons::Library::open(icon_pack()?).ok()?;
        ICONS.with(|c| c.set(Some(Box::leak(Box::new(lib)))));
    }
    ICONS.with(Cell::get)
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let js = |e: JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(js)?.dyn_into().map_err(js)?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(js)?).await.map_err(js)?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/* ------------------------------------------------------------------ clipboard */

/// Text pastes arrive as egui `Event::Paste` (eframe listens to the `paste` event): there is no synchronous read in a browser.
pub fn system_text() -> Option<String> {
    None
}

/// Images pasted into the page arrive through [`poll`] (see [`install_paste_listener`]).
pub fn system_image() -> Option<RawImage> {
    None
}

/// Puts an image (RGBA) in the system clipboard (eframe does it with the asynchronous Clipboard API).
pub fn copy_image(width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
    CTX.with(|c| match c.borrow().as_ref() {
        Some(ctx) => {
            ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba));
            Ok(())
        }
        None => Err("the page is not ready".to_owned()),
    })
}

/// eframe turns text pastes into events, but an image on the clipboard produces none: this listener reads it.
fn install_paste_listener() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let on_paste = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(|ev: web_sys::ClipboardEvent| {
        let Some(data) = ev.clipboard_data() else { return };
        // when there is text, the text wins (eframe handles it)
        if data.get_data("text/plain").is_ok_and(|t| !t.is_empty()) {
            return;
        }
        let Some(files) = data.files() else { return };
        for i in 0..files.length() {
            let Some(file) = files.get(i).filter(|f| f.type_().starts_with("image/")) else { continue };
            ev.prevent_default();
            spawn_local(async move {
                let Ok(buf) = JsFuture::from(file.array_buffer()).await else { return };
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                match image::load_from_memory(&bytes) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        push(Incoming::PastedImage((rgba.width() as usize, rgba.height() as usize, rgba.into_raw())));
                    }
                    Err(e) => web_sys::console::error_1(&format!("pasted image unreadable: {e}").into()),
                }
            });
            break;
        }
    });
    let _ = document.add_event_listener_with_callback("paste", on_paste.as_ref().unchecked_ref());
    on_paste.forget();
}

/* ------------------------------------------------------------------ invitation link, network, preferences */

/// An invitation link is `https://<page>/#<ticket>`: the ticket is in the fragment, which a browser never sends to a server. This takes it
/// out of the address bar (so that it stays neither in the history nor on a screen shared by mistake) and queues it for the application.
/// `#join=<ticket>` is read as well.
fn take_link_from_address() {
    let Some(window) = web_sys::window() else { return };
    let location = window.location();
    let hash = location.hash().unwrap_or_default();
    let link = hash.trim_start_matches('#').trim();
    if link.is_empty() {
        return;
    }
    let link = link.strip_prefix("join=").unwrap_or(link).to_owned();
    if let Ok(history) = window.history() {
        let path = format!("{}{}", location.pathname().unwrap_or_default(), location.search().unwrap_or_default());
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&path));
    }
    push(Incoming::JoinLink(link));
}

/// A link present when the page loads, or put in the address bar of the open page.
fn install_link_listener() {
    take_link_from_address();
    let Some(window) = web_sys::window() else { return };
    let on_change = Closure::<dyn FnMut()>::new(take_link_from_address);
    let _ = window.add_event_listener_with_callback("hashchange", on_change.as_ref().unchecked_ref());
    on_change.forget();
}

/// `online` and a tab that becomes visible again: the session should look at its connection now instead of waiting for its own timers
/// (a hidden tab draws no frame, so the session has been asleep). `offline` only wakes the application up to say so.
fn install_network_listeners() {
    let Some(window) = web_sys::window() else { return };
    let on_back = Closure::<dyn FnMut()>::new(|| {
        NET_EVENT.with(|e| e.set(true));
        wake();
    });
    let _ = window.add_event_listener_with_callback("online", on_back.as_ref().unchecked_ref());
    on_back.forget();
    let on_offline = Closure::<dyn FnMut()>::new(wake);
    let _ = window.add_event_listener_with_callback("offline", on_offline.as_ref().unchecked_ref());
    on_offline.forget();
    if let Some(document) = window.document() {
        let doc = document.clone();
        let on_visible = Closure::<dyn FnMut()>::new(move || {
            if !doc.hidden() {
                NET_EVENT.with(|e| e.set(true));
                wake();
            }
        });
        let _ = document.add_event_listener_with_callback("visibilitychange", on_visible.as_ref().unchecked_ref());
        on_visible.forget();
    }
}

/// Did the browser report the network back (or the tab visible again) since the last call?
pub fn take_net_event() -> bool {
    NET_EVENT.with(Cell::take)
}

/// Is the main pointer imprecise (a finger)? Then the session windows get larger touch targets.
pub fn coarse_pointer() -> bool {
    web_sys::window().and_then(|w| w.match_media("(pointer: coarse)").ok().flatten()).is_some_and(|m| m.matches())
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// A small setting (the guest's display name) kept in the browser's local storage, on this device only.
pub fn load_pref(name: &str) -> Option<String> {
    local_storage()?.get_item(&format!("inkboard.{name}")).ok().flatten()
}

pub fn save_pref(name: &str, value: &str) {
    if let Some(s) = local_storage() {
        let _ = s.set_item(&format!("inkboard.{name}"), value);
    }
}

/* ------------------------------------------------------------------ files */

/// Opens the browser's file picker; the board arrives later through [`poll`].
pub fn pick_board() -> Option<PathBuf> {
    spawn_local(async {
        let picked = rfd::AsyncFileDialog::new()
            .add_filter("Boards", &["inkboard", "json", "drawio", "excalidraw", "canvas", "mmd", "mermaid", "xml"])
            .pick_file()
            .await;
        if let Some(f) = picked {
            let bytes = f.read().await;
            push(Incoming::Board { name: f.file_name(), bytes });
        }
    });
    None
}

/// There is no save dialog in a browser (see [`DOWNLOADS`]).
pub fn pick_save_path(_name: &str, _label: &str, _exts: &[&str]) -> Option<PathBuf> {
    None
}

/// Opens the browser's file picker; the images arrive later through [`poll`].
pub fn pick_images() -> Vec<PathBuf> {
    spawn_local(async {
        let Some(files) = rfd::AsyncFileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"]).pick_files().await else { return };
        let mut items = Vec::new();
        for f in files {
            let ext = f.file_name().rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
            items.push((ext, f.read().await));
        }
        push(Incoming::Images(items));
    });
    Vec::new()
}

/// Hands a file to the browser as a download.
pub fn download(name: &str, mime: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: JsValue| format!("{e:?}");
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|e| format!("{e:?}"))?;
    a.set_href(&url);
    a.set_download(name);
    a.set_attribute("style", "display:none").map_err(js)?;
    let body = document.body().ok_or("no body")?;
    body.append_child(&a).map_err(js)?;
    a.click();
    a.remove();
    // the browser needs the URL alive while it starts the download
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
    }
    Ok(())
}

/* ------------------------------------------------------------------ window */

/// The title of the tab.
pub fn set_title(_ctx: &egui::Context, title: &str) {
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        d.set_title(title);
    }
}

/// The presentation fills the page; the browser's own full screen (F11) is the user's choice.
pub fn set_fullscreen(_ctx: &egui::Context, _on: bool) {}

/* ------------------------------------------------------------------ sessions */

/// Relay addresses of an invitation link: what the network crate decides, as on the desktop.
pub use inkboard_net::config::{check_relay_url, is_known_relay};

/// Seed for the object ids: the browser has no clock to derive it from, and two browsers must never draw the same ids.
pub fn random_seed() -> u64 {
    let mut bytes = [0u8; 8];
    if let Some(crypto) = web_sys::window().and_then(|w| w.crypto().ok())
        && crypto.get_random_values_with_u8_array(&mut bytes).is_ok()
    {
        return u64::from_le_bytes(bytes) | 1;
    }
    // no crypto (very old browser): the clock and a random float are better than a constant
    ((js_sys::Date::now() as u64) ^ ((js_sys::Math::random() * 9.0e15) as u64)) | 1
}
