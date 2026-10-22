//! Lot P: what the participants of a shared session see of each other. A host application and guest applications (kittest, one egui
//! frame each) talk through the simulated network (`SimNet`) with a virtual clock: nothing here waits for real time.
//!
//! One `pump` = a frame of every application, then 50 ms of virtual time (the network clock, the session clocks and the presence
//! timers, which are moved by `RemotePresence::skew`).

use crate::i18n::{Lang, set_for_thread};
use crate::presence::{self, Action, AgentEvent, Target};
use crate::session_guest::{Clock, GuestConfig, GuestSession, GuestState, join_url};
use crate::session_host::{HostAddress, HostTransport, SessionNet};
use crate::tests::T;
use crate::tools::Tool;
use eframe::egui::{Event, Modifiers, PointerButton};
use inkboard_core::geom::Pt;
use inkboard_core::{Id, Kind, View};
use inkboard_session::crypto::{Link, SeededEntropy};
use inkboard_session::sim::{NetConfig, SimNet};
use inkboard_session::{ParticipantId, Role};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{FakeDiagnoser, zscaler};

const STEP_MS: u64 = 50;

struct Net(SimNet);

impl SessionNet for Net {
    fn start(&mut self) -> Result<(Box<dyn HostTransport>, HostAddress), String> {
        Ok((Box::new(self.0.transport("host")), HostAddress { host: "host".to_owned(), relays: Vec::new() }))
    }
}

struct Room {
    net: SimNet,
    clock: Arc<AtomicU64>,
    host: T,
    guests: Vec<T>,
    dirs: Vec<std::path::PathBuf>,
    /// Guests whose application is not run any more: a machine or a network that vanished without a word.
    frozen: Vec<usize>,
}

impl Room {
    fn new() -> Self {
        set_for_thread(Some(Lang::En));
        let net = SimNet::new(21, NetConfig::ideal());
        let clock = Arc::new(AtomicU64::new(0));
        let mut host = T::new();
        host.app().share.net = Some(Box::new(Net(net.clone())));
        host.app().share.test_clock = Some(clock.clone());
        crate::share_ui::start(host.app());
        assert!(host.app().share.is_running());
        Room { net, clock, host, guests: Vec::new(), dirs: Vec::new(), frozen: Vec::new() }
    }

    fn pump(&mut self, n: usize) {
        for _ in 0..n {
            self.host.h.step();
            for (i, g) in self.guests.iter_mut().enumerate() {
                if !self.frozen.contains(&i) {
                    g.h.step();
                }
            }
            self.net.advance(STEP_MS);
            self.clock.store(self.net.now(), Ordering::Relaxed);
            self.host.app().agent.remote.skew += Duration::from_millis(STEP_MS);
            for g in &mut self.guests {
                g.app().agent.remote.skew += Duration::from_millis(STEP_MS);
            }
        }
    }

    /// A guest joins and is accepted by the host; returns its index.
    fn join(&mut self, name: &str, role: Role) -> usize {
        let ticket = {
            let a = self.host.app();
            let addr = a.share.addr.clone().unwrap();
            Link::parse(&a.share.host.as_ref().unwrap().link(role, &addr).unwrap()).unwrap().ticket
        };
        let k = self.guests.len() as u64;
        let dir = std::env::temp_dir().join(format!("inkboard-presence-{}", Id::new()));
        let mut t = T::new();
        let (n, c) = (self.net.clone(), self.clock.clone());
        t.app().join.factory = Some(Box::new(move |ticket, name| {
            let link = Box::new(n.transport(&format!("guest-{k}")));
            GuestSession::new(
                link,
                Arc::new(FakeDiagnoser(zscaler())),
                ticket,
                name,
                Box::new(SeededEntropy::new(100 + k)),
                Clock::Virtual(c.clone()),
                GuestConfig::default(),
            )
        }));
        t.app().join.name_dir = Some(dir.clone());
        t.app().open_join_window(Some(join_url(&ticket)));
        if let Some(w) = t.app().join.window.as_mut() {
            w.name = name.to_owned();
        }
        t.app().submit_join();
        self.dirs.push(dir);
        self.guests.push(t);
        let i = self.guests.len() - 1;
        for _ in 0..200 {
            if self.host.app().share.host.as_ref().unwrap().pending().iter().any(|p| p.name == name) {
                break;
            }
            self.pump(1);
        }
        let host = self.host.app().share.host.as_mut().unwrap();
        let request = host.pending().iter().find(|p| p.name == name).map(|p| p.request).expect("the join request");
        host.accept(request, role);
        for _ in 0..200 {
            if self.guests[i].app().guest.as_ref().is_some_and(|g| g.state() == GuestState::Live && g.mirror_ready()) {
                break;
            }
            self.pump(1);
        }
        assert!(self.guests[i].app().guest_active(), "{name} has the board");
        self.pump(4);
        i
    }

    fn id_of(&mut self, name: &str) -> ParticipantId {
        self.host.app().share.host.as_ref().unwrap().members().iter().find(|m| m.name == name).map(|m| m.id).expect("a member")
    }

    fn move_to(&mut self, guest: usize, at: (f64, f64)) {
        let p = self.guests[guest].at(at.0, at.1);
        self.guests[guest].h.event(Event::PointerMoved(p));
        self.pump(1);
    }

    fn button(&mut self, guest: usize, at: (f64, f64), pressed: bool) {
        let pos = self.guests[guest].at(at.0, at.1);
        self.guests[guest].h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        self.pump(1);
    }

    /// The guest draws with the pen up to `steps` points of a diagonal; the button stays down.
    fn draw_without_release(&mut self, guest: usize, steps: u32) {
        self.guests[guest].tool(Tool::Pen);
        self.move_to(guest, (-100.0, -60.0));
        self.button(guest, (-100.0, -60.0), true);
        for k in 1..=steps {
            self.move_to(guest, (-100.0 + 20.0 * f64::from(k), -60.0 + 8.0 * f64::from(k)));
        }
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        for d in &self.dirs {
            std::fs::remove_dir_all(d).ok();
        }
        set_for_thread(None);
    }
}

fn pen_objects(t: &mut T) -> usize {
    t.objects().iter().filter(|o| matches!(o.kind, Kind::Pen(_))).count()
}

/* ------------------------------------------------------------------------------------------------- strokes and moves */

#[test]
fn a_stroke_in_progress_is_seen_by_the_host_and_the_other_guest_then_replaced_by_the_object() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    let b = r.join("Marc", Role::Edit);
    let anna = r.id_of("Anna");
    r.draw_without_release(a, 8);
    r.pump(3);
    for (who, app) in [("host", r.host.app()), ("the other guest", r.guests[b].app())] {
        let strokes = &app.agent.remote.strokes;
        assert_eq!(strokes.len(), 1, "{who} sees the stroke in progress");
        let s = strokes.values().next().unwrap();
        assert_eq!(s.author, anna.0);
        assert!(s.pts.len() >= 5, "{who}: {} points", s.pts.len());
        assert!(app.agent.remote.people.contains_key(&anna.0), "{who} knows whose color to use");
    }
    assert_eq!(pen_objects(&mut r.host), 0, "nothing is in the board yet");
    // release: the validated object arrives, the temporary drawing goes away
    r.button(a, (60.0, 20.0), false);
    r.pump(6);
    assert!(r.host.app().agent.remote.strokes.is_empty());
    assert!(r.guests[b].app().agent.remote.strokes.is_empty());
    assert!(r.guests[a].app().agent.remote.strokes.is_empty());
    assert_eq!((pen_objects(&mut r.host), pen_objects(&mut r.guests[b])), (1, 1));
}

#[test]
fn a_move_in_progress_is_visible_to_the_host_and_the_other_guest() {
    let mut r = Room::new();
    let id = r.host.add_rect(-60.0, -40.0, 120.0, 80.0);
    r.pump(2);
    let a = r.join("Anna", Role::Edit);
    let b = r.join("Marc", Role::Edit);
    let start_x = r.host.frame(&id).x;
    r.button(a, (0.0, 0.0), true);
    for k in 1..=8 {
        r.move_to(a, (12.0 * f64::from(k), 0.0));
    }
    r.pump(3);
    let ghost_x = r.host.app().agent.remote.ghosts.get(&id).expect("the host sees the object being moved").object.frame().unwrap().x;
    assert!(ghost_x > start_x + 20.0, "{ghost_x} vs {start_x}");
    assert_eq!(r.host.frame(&id).x, start_x, "the host's board itself is not touched by the gesture");
    let seen_by_b = r.guests[b].frame(&id).x;
    assert!(seen_by_b > start_x + 20.0, "the other guest sees it moving: {seen_by_b}");
    r.button(a, (96.0, 0.0), false);
    r.pump(6);
    assert!(r.host.app().agent.remote.ghosts.is_empty(), "the ghost goes away with the gesture");
    let (h, g) = (r.host.frame(&id).x, r.guests[b].frame(&id).x);
    assert!(h > start_x + 20.0 && (h - g).abs() < 0.01, "validated: host {h}, guest {g}");
}

#[test]
fn a_stroke_cut_off_by_a_lost_connection_leaves_no_ghost() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    let b = r.join("Marc", Role::Edit);
    r.draw_without_release(a, 6);
    r.pump(2);
    assert_eq!(r.guests[b].app().agent.remote.strokes.len(), 1);
    // Anna's machine vanishes: nothing more comes, nobody says goodbye
    r.frozen.push(a);
    r.pump(3);
    assert_eq!(r.host.app().agent.remote.strokes.len(), 1, "for a moment the host cannot tell");
    assert_eq!(r.guests[b].app().agent.remote.strokes.len(), 1);
    r.pump(70); // 3.5 s of virtual time
    assert!(r.host.app().agent.remote.strokes.is_empty(), "the safety delay removed it on the host");
    assert!(r.guests[b].app().agent.remote.strokes.is_empty(), "and on the other guest");
    assert_eq!(pen_objects(&mut r.host), 0);
}

#[test]
fn a_dropped_connection_clears_the_strokes_of_the_host_at_once() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    r.draw_without_release(a, 6);
    r.pump(2);
    assert_eq!(r.host.app().agent.remote.strokes.len(), 1);
    r.guests[a].app().guest.as_mut().unwrap().cut_connection();
    r.pump(2);
    assert!(r.host.app().agent.remote.strokes.is_empty(), "the host noticed the connection dropped");
}

#[test]
fn a_gesture_held_still_is_not_taken_for_an_abandoned_one() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    r.draw_without_release(a, 4);
    r.pump(80); // 4 s without a new point, but Anna's presence keeps arriving
    assert_eq!(r.host.app().agent.remote.strokes.len(), 1);
    r.button(a, (-20.0, -28.0), false);
}

#[test]
fn guests_send_at_most_20_messages_of_each_kind_per_second_and_the_host_does_not_complain() {
    let mut r = Room::new();
    let ids: Vec<Id> = (0..5).map(|i| r.host.add_rect(-200.0 + 90.0 * f64::from(i), -60.0, 70.0, 50.0)).collect();
    let a = r.join("Anna", Role::Edit);
    // 1) a long stroke
    r.draw_without_release(a, 40);
    r.button(a, (700.0, 260.0), false);
    // 2) five objects moved together
    r.guests[a].tool(Tool::Select);
    r.guests[a].app().sel = ids.clone();
    r.button(a, (-165.0, -35.0), true);
    for k in 1..=40 {
        r.move_to(a, (-165.0 + 3.0 * f64::from(k), -35.0 + f64::from(k)));
    }
    r.button(a, (-45.0, 5.0), false);
    r.pump(6);
    let log = r.guests[a].app().guest.as_ref().unwrap().sent_log.clone();
    for kind in ["presence", "stroke", "transient"] {
        let times: Vec<u64> = log.iter().filter(|(_, k)| *k == kind).map(|(t, _)| *t).collect();
        assert!(!times.is_empty(), "no {kind} message was sent at all");
        for (i, t0) in times.iter().enumerate() {
            let in_window = times[i..].iter().take_while(|t| **t < t0 + 1000).count();
            assert!(in_window <= 21, "{in_window} {kind} messages within one virtual second");
        }
    }
    let host = r.host.app().share.host.as_ref().unwrap();
    assert!(host.members().iter().any(|m| m.name == "Anna" && m.connected), "the host did not exclude the guest");
}

/* -------------------------------------------------------------------------------------------------- editing badge */

#[test]
fn the_typing_badge_appears_on_the_object_being_edited_and_disappears() {
    let mut r = Room::new();
    let id = r.host.add_rect(-60.0, -40.0, 120.0, 80.0);
    r.pump(2);
    let a = r.join("Anna", Role::Edit);
    let b = r.join("Marc", Role::Edit);
    let anna = r.id_of("Anna");
    let editing = |t: &mut T| t.app().agent.remote.people.get(&anna.0).and_then(|p| p.editing.clone()).map(|(i, _)| i);
    r.guests[a].app().start_editing(&id, None);
    r.pump(4);
    assert_eq!(editing(&mut r.host), Some(id.clone()), "the host sees who is typing where");
    assert_eq!(editing(&mut r.guests[b]), Some(id.clone()));
    // the badge is informative only: the other guest can still start editing the same object
    r.guests[b].app().start_editing(&id, None);
    r.pump(4);
    assert!(r.guests[b].app().editing.is_some());
    r.guests[b].app().commit_editing();
    r.guests[a].app().commit_editing();
    r.pump(4);
    assert_eq!(editing(&mut r.host), None);
    assert_eq!(editing(&mut r.guests[b]), None);
    // a connection lost while typing: nobody renews the news, the badge fades by itself
    r.guests[a].app().start_editing(&id, None);
    r.pump(4);
    assert_eq!(editing(&mut r.guests[b]), Some(id));
    r.frozen.push(a);
    r.pump(2);
    r.pump(130); // 6.5 s
    assert_eq!(editing(&mut r.guests[b]), None);
}

#[test]
fn the_host_typing_is_shown_to_the_guests_too() {
    let mut r = Room::new();
    let id = r.host.add_rect(-60.0, -40.0, 120.0, 80.0);
    let a = r.join("Anna", Role::Edit);
    r.host.app().start_editing(&id, None);
    for _ in 0..200 {
        r.pump(1);
        if r.guests[a].app().agent.remote.people.get(&0).is_some_and(|p| p.editing.is_some()) {
            return;
        }
    }
    panic!("the guest never saw the host typing");
}

/* ------------------------------------------------------------------------------------------------------- follow */

fn center_of_view(t: &mut T) -> Pt {
    let v = t.app().ed.page().view;
    let (w, h) = (f64::from(t.app().canvas_size.0), f64::from(t.app().canvas_size.1));
    Pt::new((w / 2.0 - v.x) / v.zoom, (h / 2.0 - v.y) / v.zoom)
}

#[test]
fn follow_moves_the_view_to_the_participants_and_stops_when_the_user_acts() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    let anna = r.id_of("Anna");
    // Anna looks elsewhere, at another zoom
    r.guests[a].app().set_view(View { x: -900.0, y: -500.0, zoom: 2.0 });
    r.pump(4);
    let target = center_of_view(&mut r.guests[a]);
    assert!(center_of_view(&mut r.host).dist(target) > 100.0);
    presence::follow(r.host.app(), Some(anna.0));
    for _ in 0..80 {
        r.pump(1);
    }
    let (here, zoom) = (center_of_view(&mut r.host), r.host.app().ed.page().view.zoom);
    assert!(here.dist(target) < 2.0 && (zoom - 2.0).abs() < 0.05, "host looks at {here:?} (zoom {zoom}), Anna at {target:?}");
    assert_eq!(r.host.app().agent.remote.following, Some(anna.0), "still following");
    // Anna moves again: the host follows
    r.guests[a].app().set_view(View { x: 300.0, y: 100.0, zoom: 1.0 });
    r.pump(4);
    let target = center_of_view(&mut r.guests[a]);
    for _ in 0..80 {
        r.pump(1);
    }
    assert!(center_of_view(&mut r.host).dist(target) < 2.0);
    // a click on the board ends it, and the view stays where the user is
    let p = r.host.at(0.0, 0.0);
    r.host.h.event(Event::PointerMoved(p));
    r.pump(1);
    r.host.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    r.pump(1);
    assert_eq!(r.host.app().agent.remote.following, None);
    r.host.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    r.pump(1);
    r.guests[a].app().set_view(View { x: -50.0, y: -50.0, zoom: 3.0 });
    let kept = r.host.app().ed.page().view;
    r.pump(10);
    assert_eq!(r.host.app().ed.page().view, kept, "the host's view does not move any more");
}

#[test]
fn moving_the_view_by_hand_ends_the_following() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    let anna = r.id_of("Anna");
    r.guests[a].app().set_view(View { x: -400.0, y: -300.0, zoom: 1.5 });
    r.pump(4);
    presence::follow(r.host.app(), Some(anna.0));
    r.pump(3);
    // the user zooms with the wheel over the board
    let p = r.host.app().canvas_rect.center();
    r.host.h.event(Event::PointerMoved(p));
    r.pump(1);
    let before = r.host.app().ed.page().view;
    r.host.h.event(Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: eframe::egui::vec2(0.0, 120.0), modifiers: Modifiers::NONE });
    r.pump(3);
    assert_ne!(r.host.app().ed.page().view, before, "the wheel did move the view");
    assert_eq!(r.host.app().agent.remote.following, None);
}

#[test]
fn a_guest_can_follow_the_host_and_the_stop_button_ends_it() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    r.host.app().set_view(View { x: -700.0, y: -400.0, zoom: 1.0 });
    presence::follow(r.guests[a].app(), Some(ParticipantId::HOST.0));
    let mut done = false;
    for _ in 0..200 {
        r.pump(1);
        let (hc, gc) = (center_of_view(&mut r.host), center_of_view(&mut r.guests[a]));
        if hc.dist(gc) < 2.0 {
            done = true;
            break;
        }
    }
    assert!(done, "the guest's view reached the host's");
    assert_eq!(r.guests[a].app().agent.remote.following, Some(0));
    use egui_kittest::kittest::Queryable;
    let buttons = r.guests[a].h.get_all_by_label("Stop following").count();
    assert!(buttons >= 1);
    r.guests[a].h.get_all_by_label("Stop following").last().unwrap().click();
    r.pump(3);
    assert_eq!(r.guests[a].app().agent.remote.following, None);
}

/* ------------------------------------------------------------------------------------------------------ the agent */

#[test]
fn the_agent_of_the_host_is_seen_acting_by_the_guests() {
    let mut r = Room::new();
    let a = r.join("Anna", Role::Edit);
    // the agent creates an object and the event is recorded in the same frame (as a tool call does)
    let id = r.host.add_rect(100.0, 60.0, 120.0, 80.0);
    let page = r.host.app().ed.page().clone();
    let frame = page.bounds(page.get(&id).unwrap());
    let now = Instant::now();
    let ev = AgentEvent { name: "Claude".into(), tool: "add_shape".into(), action: Action::Act, steps: vec![vec![Target { id: id.clone(), frame }]], at: now };
    r.host.app().agent.record(ev, Pt::ZERO, now);
    r.pump(4);
    let g = r.guests[a].app();
    assert_eq!(g.agent.state.participant.name, "Claude");
    assert!(g.agent.state.cursor.is_some(), "the agent's cursor is shown");
    assert!(g.agent.state.halo_count() > 0, "with a halo on the object it created");
    assert!(g.ed.page().get(&id).is_some());
}

/* ------------------------------------------------------------------------------------------------- screenshots */

fn save(t: &mut T, name: &str) {
    for _ in 0..3 {
        t.h.step();
    }
    let image = t.h.render().unwrap();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../context/mcp-remote").join(name);
    image.save(out).unwrap();
}

/// `cargo +1.99.1 test -p inkboard-app capture_lot_p -- --ignored` writes `context/mcp-remote/lot-p-*.png`.
#[test]
#[ignore = "writes screenshots"]
fn capture_lot_p_screenshots() {
    let mut r = Room::new();
    let id = r.host.add_rect(-60.0, -40.0, 160.0, 90.0);
    r.pump(2);
    let a = r.join("Anna", Role::Edit);
    r.draw_without_release(a, 12);
    r.pump(3);
    r.host.app().share.open = false;
    save(&mut r.host, "lot-p-stroke.png");
    r.button(a, (140.0, 36.0), false);
    r.pump(6);
    r.guests[a].app().start_editing(&id, None);
    r.pump(6);
    save(&mut r.host, "lot-p-badge.png");
}
