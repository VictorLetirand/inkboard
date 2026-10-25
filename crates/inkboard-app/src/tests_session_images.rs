//! Images in shared sessions (lot G): a guest sends the images it adds (files, drops, pastes, icons, formulas) to the host piece by
//! piece, and everybody gets them by hash. The real `SessionHost` and real `GuestSession`s (inside kittest applications) talk over the
//! simulated network with a virtual clock: no socket, no sleep, no assertion about durations. Loops are bounded by a number of rounds.
//! This module is included by `tests_session_guest.rs` (it reuses its small hostile host for the images a guest receives).

use super::World as FakeWorld;
use crate::app::App;
use crate::i18n::{Lang, set_for_thread};
use crate::render::decode_asset;
use crate::session_guest::{Clock, GuestConfig, GuestSession, GuestState, NoDiagnosis, unreadable_asset};
use crate::session_host::{HostAddress, HostConfig, HostTransport, SessionHost};
use crate::tests::T;
use inkboard_core::Editor;
use inkboard_core::geom::Frame;
use inkboard_core::model::{Document, Id, ImageObj, Kind, Object};
use inkboard_core::style::Style;
use inkboard_session::assets::{chunks, hash_hex};
use inkboard_session::crypto::SeededEntropy;
use inkboard_session::proto::{AssetInfo, Role};
use inkboard_session::sim::{NetConfig, SimNet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Size of the pieces a guest sends (`UPLOAD_CHUNK` of `session_guest.rs`).
const PIECE: usize = 128 << 10;

/* ------------------------------------------------------------------ data */

/// A PNG of random pixels (they do not compress: the file is about `w * h * 4` bytes).
fn noise_png(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let img = image::RgbaImage::from_fn(w, h, |_, _| {
        let v = next();
        image::Rgba([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255])
    });
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A tiny file that declares a 60 000 x 60 000 bitmap (a decompression bomb if it were decoded).
fn bomb_png() -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&60_000u32.to_be_bytes());
    ihdr.extend_from_slice(&60_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(&b"IHDR"[..], ihdr), (&b"IDAT"[..], vec![0x78, 0x9c]), (&b"IEND"[..], Vec::new())] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut typed = kind.to_vec();
        typed.extend_from_slice(&body);
        out.extend_from_slice(&typed);
        out.extend_from_slice(&crc32(&typed).to_be_bytes());
    }
    out
}

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>"#;

fn images_of(ed: &Editor) -> Vec<ImageObj> {
    ed.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Image(i) = &o.kind { Some(i.clone()) } else { None }).collect()
}

/// Is there an image object on the first page, with these exact bytes under the asset named by the object?
fn shows(ed: &Editor, bytes: &[u8]) -> bool {
    images_of(ed).iter().any(|i| ed.doc.assets.get(&i.asset).is_some_and(|a| *a.data == bytes))
}

/// An image object put in the user's editor without going through a file (what the file dialog does after decoding).
fn insert_raw(t: &mut T, mime: &str, bytes: Vec<u8>) -> Id {
    let app = t.app();
    let aid = app.ed.doc.add_asset(mime, bytes);
    let style = Style { width: 0.0, fill: None, ..Style::default() };
    app.ed.insert(Object::new(Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 100.0, 80.0), asset: aid, latex: None, mono: false }), style))
}

fn toast(t: &mut T) -> String {
    t.app().toast.as_ref().map(|(m, _)| m.clone()).unwrap_or_default()
}

/* ------------------------------------------------------------------ a room: the real host and applications as guests */

struct Room {
    net: SimNet,
    clock: Arc<AtomicU64>,
    ed: Editor,
    host: SessionHost,
    addr: HostAddress,
    guests: Vec<T>,
    dirs: Vec<std::path::PathBuf>,
}

impl Room {
    fn new(seed: u64, tweak: impl FnOnce(&mut HostConfig)) -> Self {
        set_for_thread(Some(Lang::En));
        let net = SimNet::new(seed, NetConfig::ideal());
        let ed = Editor::new(Document::new("board"));
        let mut cfg = HostConfig { entropy_seed: Some(seed ^ 0xA5A5), ..HostConfig::default() };
        tweak(&mut cfg);
        let host = SessionHost::start(cfg, &ed, Box::new(net.transport("host")) as Box<dyn HostTransport>).unwrap();
        let addr = HostAddress { host: "host".to_owned(), relays: Vec::new() };
        Room { net, clock: Arc::new(AtomicU64::new(0)), ed, host, addr, guests: Vec::new(), dirs: Vec::new() }
    }

    /// One round: 50 ms of virtual time, the host, one frame of every application.
    fn round(&mut self) {
        self.net.advance(50);
        let now = self.net.now();
        self.clock.store(now, Ordering::Relaxed);
        self.host.poll(&mut self.ed, now);
        for g in &mut self.guests {
            g.h.step();
        }
    }

    fn rounds(&mut self, n: usize) {
        for _ in 0..n {
            self.round();
        }
    }

    fn until(&mut self, what: &str, mut cond: impl FnMut(&mut Room) -> bool) {
        for _ in 0..800 {
            if cond(self) {
                return;
            }
            self.round();
        }
        panic!("gave up waiting for: {what}");
    }

    fn join(&mut self, name: &str, role: Role) -> usize {
        let link = self.host.link(role, &self.addr).unwrap();
        let idx = self.guests.len();
        let mut t = T::new();
        let (net, clock) = (self.net.clone(), self.clock.clone());
        t.app().join.factory = Some(Box::new(move |ticket, name| {
            let link = Box::new(net.transport(&format!("guest-{idx}")));
            GuestSession::new(
                link,
                Arc::new(NoDiagnosis),
                ticket,
                name,
                Box::new(SeededEntropy::new(3 + idx as u64)),
                Clock::Virtual(clock.clone()),
                GuestConfig::default(),
            )
        }));
        let dir = std::env::temp_dir().join(format!("inkboard-img-{}", Id::new()));
        t.app().join.name_dir = Some(dir.clone());
        t.app().open_join_window(Some(link));
        if let Some(w) = t.app().join.window.as_mut() {
            w.name = name.to_owned();
        }
        t.app().submit_join();
        self.dirs.push(dir);
        self.guests.push(t);
        self.until(&format!("the request of {name}"), |r| r.host.pending().iter().any(|p| p.name == name));
        let request = self.host.pending().iter().find(|p| p.name == name).map(|p| p.request).unwrap();
        self.host.accept(request, role);
        self.until(&format!("{name} to have the board"), |r| r.session(idx).state() == GuestState::Live && r.session(idx).mirror_ready());
        idx
    }

    fn session(&mut self, guest: usize) -> &mut GuestSession {
        self.guests[guest].app().guest.as_mut().expect("a guest session")
    }

    fn app(&mut self, guest: usize) -> &mut App {
        self.guests[guest].app()
    }

    /// Image files chosen by a guest in the file dialog.
    fn insert_files(&mut self, guest: usize, files: &[(&str, &[u8])]) {
        let dir = std::env::temp_dir().join(format!("inkboard-img-files-{}", Id::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<std::path::PathBuf> = files
            .iter()
            .map(|(name, bytes)| {
                let p = dir.join(name);
                std::fs::write(&p, bytes).unwrap();
                p
            })
            .collect();
        self.dirs.push(dir);
        let app = self.app(guest);
        let at = app.view_center();
        app.insert_image_files(&paths, at);
    }

    fn host_images(&self) -> usize {
        images_of(&self.ed).len()
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        for d in &self.dirs {
            let _ = std::fs::remove_dir_all(d);
        }
        set_for_thread(None);
    }
}

/* ------------------------------------------------------------------ reaching the others */

#[test]
fn an_image_added_by_a_guest_shows_on_the_host_and_on_a_second_guest() {
    let mut r = Room::new(1, |_| {});
    let (a, b) = (r.join("Anna", Role::Edit), r.join("Marc", Role::Edit));
    let png = noise_png(160, 120, 1);
    let hash = hash_hex(&png);
    r.insert_files(a, &[("photo.png", &png)]);
    r.until("the image on the host and on the second guest", |r| {
        shows(&r.ed, &png) && shows(&r.guests[b].app().ed, &png) && shows(&r.guests[a].app().ed, &png)
    });
    // the identifier is the hash of the bytes, everywhere
    let docs = [r.ed.doc.clone(), r.app(a).ed.doc.clone(), r.app(b).ed.doc.clone()];
    for doc in &docs {
        let objects = &doc.pages[0].objects;
        assert!(objects.iter().all(|o| matches!(&o.kind, Kind::Image(i) if i.asset.as_str() == hash)));
        assert!(decode_asset(&doc.assets[&Id::from(hash.as_str())]).is_some(), "the image can be decoded");
    }
    assert_eq!(r.host.members().len(), 2);
    assert!(!r.session(a).is_uploading());
    assert_eq!(r.session(a).pending_len(), 0);
}

#[test]
fn an_icon_placed_by_a_guest_shows_on_the_host_and_on_a_second_guest() {
    let mut r = Room::new(2, |_| {});
    let (a, b) = (r.join("Anna", Role::Edit), r.join("Marc", Role::Edit));
    let lib = inkboard_icons::Library::open(inkboard_icons::PACK).unwrap();
    let icon = lib.search("router", 1)[0];
    r.app(a).insert_icon(&lib, icon);
    r.until("the icon everywhere", |r| {
        let svg = |ed: &Editor| images_of(ed).iter().any(|i| ed.doc.assets.get(&i.asset).is_some_and(|x| x.mime == "image/svg+xml"));
        svg(&r.ed) && svg(&r.guests[b].app().ed)
    });
    assert_eq!(r.host_images(), 1);
}

#[test]
fn a_formula_placed_by_a_guest_shows_on_the_host_and_on_a_second_guest() {
    let mut r = Room::new(3, |_| {});
    let (a, b) = (r.join("Anna", Role::Edit), r.join("Marc", Role::Edit));
    r.app(a).insert_formula("x^2", None).unwrap();
    r.until("the formula everywhere", |r| {
        let formula = |ed: &Editor| {
            images_of(ed).iter().any(|i| i.latex.as_deref() == Some("x^2") && ed.doc.assets.get(&i.asset).is_some_and(|x| decode_asset(x).is_some()))
        };
        formula(&r.ed) && formula(&r.guests[b].app().ed)
    });
}

#[test]
fn dropped_files_including_svg_and_a_pasted_image_all_reach_the_host() {
    let mut r = Room::new(4, |_| {});
    let a = r.join("Anna", Role::Edit);
    let (one, two) = (noise_png(64, 48, 10), noise_png(80, 40, 11));
    let dir = std::env::temp_dir().join(format!("inkboard-img-drop-{}", Id::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = Vec::new();
    for (name, bytes) in [("one.png", &one[..]), ("icon.SVG", SVG), ("two.png", &two[..])] {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        files.push(eframe::egui::DroppedFile { path: Some(p), ..Default::default() });
    }
    r.dirs.push(dir);
    r.guests[a].h.input_mut().dropped_files = files;
    r.round();
    r.guests[a].h.input_mut().dropped_files.clear();
    r.until("the three dropped images", |r| r.host_images() == 3);
    assert!(shows(&r.ed, &one) && shows(&r.ed, &two) && shows(&r.ed, SVG));
    // Ctrl+V with an image on the clipboard
    r.app(a).system_image = || Some((8, 8, vec![200; 8 * 8 * 4]));
    r.app(a).paste_event("");
    r.until("the pasted image", |r| r.host_images() == 4);
    assert_eq!(r.ed.doc.assets.len(), 4);
}

/* ------------------------------------------------------------------ limits */

#[test]
fn an_image_over_the_size_limit_is_cancelled_locally_with_a_message() {
    let mut r = Room::new(5, |_| {});
    let a = r.join("Anna", Role::Edit);
    let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
    big.resize((15 << 20) + 1, 7);
    insert_raw(&mut r.guests[a], "image/png", big);
    r.rounds(20);
    assert!(toast(&mut r.guests[a]).contains("Image too large"), "{}", toast(&mut r.guests[a]));
    assert!(images_of(&r.app(a).ed).is_empty(), "the image is gone from the guest's board");
    assert!(r.app(a).ed.doc.assets.is_empty());
    assert_eq!(r.session(a).chunks_sent, 0, "nothing was sent");
    assert_eq!((r.host_images(), r.ed.doc.assets.len()), (0, 0));
    assert_eq!(r.host.members().len(), 1, "not a violation: the guest was not removed");
    // the guest goes on working
    let rect = r.guests[a].add_rect(0.0, 0.0, 50.0, 50.0);
    r.until("the next shape to reach the host", |r| r.ed.doc.pages[0].get(&rect).is_some());
}

#[test]
fn a_vector_image_with_a_filter_is_cancelled_locally_with_a_message() {
    let mut r = Room::new(12, |_| {});
    let a = r.join("Anna", Role::Edit);
    let heavy = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><filter id="f"><feGaussianBlur stdDeviation="300"/></filter><rect width="20" height="20" filter="url(#f)"/></svg>"##;
    insert_raw(&mut r.guests[a], "image/svg+xml", heavy.to_vec());
    r.rounds(20);
    assert!(toast(&mut r.guests[a]).contains("not allowed"), "{}", toast(&mut r.guests[a]));
    assert!(images_of(&r.app(a).ed).is_empty(), "the image is gone from the guest's board");
    assert_eq!((r.session(a).chunks_sent, r.host_images()), (0, 0), "nothing was sent");
    assert_eq!(r.host.members().len(), 1);
}

#[test]
fn a_texture_is_never_larger_than_the_graphics_limit() {
    use crate::render::decode_asset_within;
    use inkboard_core::model::Asset;
    for (w, h) in [(20_000, 1), (1, 20_000), (9_000, 300)] {
        let asset = Asset { mime: "image/png".to_owned(), data: Arc::new(noise_png(w, h, 3)) };
        for max in [2048usize, 8192] {
            let img = decode_asset_within(&asset, max).unwrap_or_else(|| panic!("{w}x{h} cannot be shown"));
            assert!(img.size[0] >= 1 && img.size[1] >= 1 && img.size[0] <= max && img.size[1] <= max, "{w}x{h} -> {:?} for {max}", img.size);
        }
        let img = decode_asset(&asset).expect("default limit");
        assert!(img.size[0] <= 4096 && img.size[1] <= 4096);
    }
    // an image that is already small is not touched
    let small = Asset { mime: "image/png".to_owned(), data: Arc::new(noise_png(30, 20, 4)) };
    assert_eq!(decode_asset_within(&small, 8192).unwrap().size, [30, 20]);
    // a tiny file declaring 60 000 x 60 000 pixels is not decoded
    let bomb = Asset { mime: "image/png".to_owned(), data: Arc::new(bomb_png()) };
    assert!(decode_asset_within(&bomb, 8192).is_none());
    // a huge vector image is rasterized within the limit too
    let wide = Asset {
        mime: "image/svg+xml".to_owned(),
        data: Arc::new(br#"<svg xmlns="http://www.w3.org/2000/svg" width="40000" height="10"><rect width="40000" height="10"/></svg>"#.to_vec()),
    };
    assert!(decode_asset_within(&wide, 8192).is_some_and(|i| i.size[0] <= 8192));
}

#[test]
fn an_image_over_the_quota_is_refused_by_the_host_and_cancelled_on_the_guest() {
    let (first, second) = (noise_png(120, 90, 20), noise_png(120, 90, 21));
    let quota = (first.len() + 100) as u64;
    let mut r = Room::new(6, |c| c.limits.guest_asset_quota_bytes = quota);
    let a = r.join("Anna", Role::Edit);
    r.insert_files(a, &[("first.png", &first)]);
    r.until("the first image", |r| shows(&r.ed, &first));
    r.insert_files(a, &[("second.png", &second)]);
    r.until("the refusal", |r| toast(&mut r.guests[a]).contains("quota"));
    r.rounds(10);
    assert_eq!(r.host_images(), 1, "only the first image is on the host");
    assert!(!shows(&r.ed, &second));
    assert!(!shows(&r.app(a).ed, &second), "the second image was taken off the guest's board");
    assert!(shows(&r.app(a).ed, &first));
    assert_eq!(r.session(a).pending_len(), 0, "nothing waits any more");
    assert!(!r.session(a).is_uploading());
    assert_eq!(r.host.members().len(), 1, "a refused image is a warning, not an exclusion");
}

#[test]
fn the_same_image_three_times_is_sent_once() {
    let mut r = Room::new(7, |_| {});
    let a = r.join("Anna", Role::Edit);
    let png = noise_png(300, 250, 30);
    let pieces = png.len().div_ceil(PIECE) as u64;
    assert!(pieces >= 2, "the image needs several pieces");
    r.insert_files(a, &[("p.png", &png)]);
    r.round();
    r.insert_files(a, &[("p.png", &png)]); // while the first is still being sent
    r.until("two images on the host", |r| r.host_images() == 2);
    r.insert_files(a, &[("p.png", &png)]); // once the host has it
    r.until("three images on the host", |r| r.host_images() == 3);
    assert_eq!(r.session(a).chunks_sent, pieces, "one transfer");
    assert_eq!(r.ed.doc.assets.len(), 1);
}

#[test]
fn an_image_the_host_already_has_is_not_sent_again() {
    let mut r = Room::new(8, |_| {});
    let png = noise_png(120, 100, 40);
    // the host's own image (an identifier that is not a hash)
    let aid = r.ed.doc.add_asset("image/png", png.clone());
    let obj = Object::new(
        Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 60.0, 50.0), asset: aid.clone(), latex: None, mono: false }),
        Style { width: 0.0, fill: None, ..Style::default() },
    );
    r.ed.insert(obj);
    let a = r.join("Anna", Role::Edit);
    r.until("the guest to have the host's image", |r| shows(&r.guests[a].app().ed, &png));
    r.insert_files(a, &[("same.png", &png)]);
    r.until("the second object on the host", |r| r.host_images() == 2);
    assert_eq!(r.session(a).chunks_sent, 0, "known by its hash: nothing to send");
    assert_eq!(r.ed.doc.assets.len(), 1);
    assert!(images_of(&r.ed).iter().all(|i| i.asset == aid), "both objects use the host's image");
}

#[test]
fn an_image_the_host_adds_after_the_guest_joined_reaches_it_without_a_new_snapshot() {
    let mut r = Room::new(12, |_| {});
    let a = r.join("Anna", Role::Edit);
    let b = r.join("Marc", Role::View);
    r.rounds(10);
    let png = noise_png(120, 100, 41);
    // the host places an image (an identifier that is not a hash) once everybody is in
    let aid = r.ed.doc.add_asset("image/png", png.clone());
    let obj = Object::new(
        Kind::Image(ImageObj { frame: Frame::new(0.0, 0.0, 60.0, 50.0), asset: aid, latex: None, mono: false }),
        Style { width: 0.0, fill: None, ..Style::default() },
    );
    r.ed.insert(obj);
    r.until("both guests to show the host's new image", |r| shows(&r.guests[a].app().ed, &png) && shows(&r.guests[b].app().ed, &png));
    assert_eq!(r.session(a).chunks_sent, 0);
    assert!(r.session(a).state() == GuestState::Live, "no reconnection was needed");
}

#[test]
fn the_images_of_a_board_that_arrives_compressed_reach_a_late_guest() {
    // the snapshot of a board with a few hundred objects is sent packed (protocol v3): the list of images it carries must still be read
    let mut r = Room::new(13, |_| {});
    let png = noise_png(90, 70, 77);
    let aid = r.ed.doc.add_asset("image/png", png.clone());
    for i in 0..300 {
        let mut o =
            Object::new_box(inkboard_core::BoxGeom::Rect, Frame::new(f64::from(i) * 20.0, 0.0, 80.0, 40.0), Style::default(), inkboard_core::Label::default());
        o.id = Id(format!("filler{i}"));
        r.ed.insert(o);
    }
    r.ed.insert(Object::new(
        Kind::Image(ImageObj { frame: Frame::new(0.0, 100.0, 60.0, 50.0), asset: aid, latex: None, mono: false }),
        Style { width: 0.0, fill: None, ..Style::default() },
    ));
    r.rounds(3);
    let a = r.join("Anna", Role::Edit);
    r.until("the late guest to show the image", |r| shows(&r.guests[a].app().ed, &png));
    assert_eq!(r.guests[a].app().ed.doc.pages[0].objects.len(), 301);
}

#[test]
fn a_reader_cannot_upload() {
    let mut r = Room::new(9, |_| {});
    let v = r.join("Viewer", Role::View);
    assert!(r.app(v).view_only());
    insert_raw(&mut r.guests[v], "image/png", noise_png(40, 30, 50));
    r.insert_files(v, &[("x.png", &noise_png(40, 30, 51))]);
    r.guests[v].h.input_mut().dropped_files = vec![eframe::egui::DroppedFile { path: Some(std::env::temp_dir().join("nothing.png")), ..Default::default() }];
    r.round();
    r.guests[v].h.input_mut().dropped_files.clear();
    r.app(v).system_image = || Some((4, 4, vec![9; 64]));
    r.app(v).paste_event("");
    r.rounds(30);
    assert!(images_of(&r.app(v).ed).is_empty(), "the reader's own editor refuses");
    assert_eq!(r.session(v).chunks_sent, 0);
    assert_eq!((r.host_images(), r.ed.doc.assets.len()), (0, 0));
    assert_eq!(r.host.members().len(), 1);
}

/* ------------------------------------------------------------------ outage, corruption */

#[test]
fn a_cut_in_the_middle_of_a_transfer_resumes_without_a_duplicate() {
    let mut r = Room::new(10, |_| {});
    let a = r.join("Anna", Role::Edit);
    let png = noise_png(600, 500, 60);
    let pieces = png.len().div_ceil(PIECE) as u64;
    assert!(pieces >= 8, "a long transfer ({pieces} pieces)");
    r.insert_files(a, &[("big.png", &png)]);
    r.until("the transfer to be half done", |r| r.session(a).upload_progress().is_some_and(|p| p.percent >= 40));
    assert!(!shows(&r.ed, &png), "not finished yet");
    let before = r.session(a).participant();
    r.session(a).cut_connection();
    let mut reconnecting = false;
    r.until("the image on the host after the reconnection", |r| {
        reconnecting |= r.session(a).state() == GuestState::Reconnecting;
        shows(&r.ed, &png)
    });
    assert!(reconnecting, "the connection really dropped");
    r.rounds(10);
    assert_eq!(r.session(a).participant(), before, "the same place");
    assert_eq!(r.host.members().len(), 1);
    assert_eq!((r.host_images(), r.ed.doc.assets.len()), (1, 1), "no duplicate");
    assert!(r.session(a).chunks_sent <= pieces + 5, "it went on from what the host held: {} pieces sent for {pieces}", r.session(a).chunks_sent);
    assert_eq!(r.session(a).pending_len(), 0);
}

#[test]
fn a_corrupted_image_does_not_crash_anything_and_shows_as_unreadable() {
    let mut r = Room::new(11, |_| {});
    let (a, b) = (r.join("Anna", Role::Edit), r.join("Marc", Role::Edit));
    // a real header, a body cut in half: accepted (the host never decodes), unreadable when drawn
    let png = noise_png(100, 80, 70);
    let cut = png[..png.len() / 2].to_vec();
    insert_raw(&mut r.guests[a], "image/png", cut.clone());
    r.until("the damaged image on the host and on the second guest", |r| shows(&r.ed, &cut) && shows(&r.guests[b].app().ed, &cut));
    for ed in [&r.ed, &r.guests[b].app().ed] {
        let i = images_of(ed).remove(0);
        assert!(decode_asset(&ed.doc.assets[&i.asset]).is_none(), "cannot be decoded");
    }
    r.rounds(5); // drawn without panicking
    // a tiny file that declares 60 000 x 60 000 pixels never leaves the guest
    insert_raw(&mut r.guests[a], "image/png", bomb_png());
    r.rounds(20);
    assert_eq!(toast(&mut r.guests[a]), crate::i18n::t("Unreadable image"));
    assert_eq!(r.host_images(), 1);
    assert_eq!(r.host.members().len(), 2);
}

#[test]
fn an_image_that_does_not_match_its_hash_is_replaced_by_a_marker() {
    let mut w = FakeWorld::new(50);
    let good = noise_png(30, 30, 80);
    let broken = {
        let mut b = good.clone();
        let n = b.len() - 20;
        b[n] ^= 0xFF;
        b
    };
    let (good_hash, broken_hash) = (hash_hex(&good), hash_hex(&b"a different content"[..]));
    let info = |id: &str, hash: &str, size: usize| AssetInfo { id: id.into(), hash: hash.to_owned(), size: size as u64, mime: "image/png".into() };
    w.host.manifest = vec![info("fine", &good_hash, good.len()), info("bad", &broken_hash, broken.len())];
    w.host.blobs.insert(good_hash.clone(), chunks(&good, &good_hash, 100));
    w.host.blobs.insert(broken_hash.clone(), chunks(&broken, &broken_hash, 100));
    w.join(Role::Edit, "Marc");
    w.pump(60);
    let mut doc = Document::default();
    w.app().guest.as_ref().expect("a session").attach_images(&mut doc);
    assert_eq!(*doc.assets[&Id::from("fine")].data, good, "the good image is kept");
    assert_eq!(*doc.assets[&Id::from("bad")].data, *unreadable_asset().data, "the other one is the marker");
    assert!(doc.assets[&Id::from("bad")].mime == "image/svg+xml");
    assert!(decode_asset(&doc.assets[&Id::from("bad")]).is_some(), "the marker itself can be drawn");
}
