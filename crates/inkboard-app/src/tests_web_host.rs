//! Host process of the browser end-to-end bench (`web/test/e2e.mjs`). It is a test only to reuse the headless application harness; it is
//! `#[ignore]`d, so `cargo test` never runs it, and the bench starts it by name. It shares a board of a few objects through the PUBLIC
//! relays (Internet needed), accepts every request to join with the right to edit, and obeys the bench through its standard input.
//!
//! Standard output (one line each): `LINK <link>` (edit), `LINK_READ <link>` (read-only), `OBJECTS <n>`, `MEMBER <name> <role>` (a request
//! to join, accepted with what its link allows), `ADDED <n>`, `COUNT <n>` (objects), `IMAGES <n>`, `PEOPLE <names>` (connected guests,
//! comma separated), `AGENT` (an agent event was sent). Standard input: `add` (adds an object), `count`, `images`, `people`, `agent` (the
//! MCP agent "Claude" acts on the first object: guests see its cursor), `quit`.

use crate::i18n::{Lang, set_for_thread};
use crate::presence::{Action, AgentEvent, Target};
use crate::session_net::IrohHostNet;
use crate::tests::T;
use inkboard_core::geom::Pt;
use inkboard_net::NetConfig;
use inkboard_session::proto::Role;
use std::io::BufRead;
use std::sync::mpsc::channel;
use std::time::Duration;

const OBJECTS: usize = 5;

fn objects(host: &mut T) -> usize {
    host.app().ed.doc.pages.iter().map(|p| p.objects.len()).sum()
}

#[test]
#[ignore = "host process of the browser bench (web/test/e2e.mjs); needs Internet"]
fn web_bench_host() {
    set_for_thread(Some(Lang::En));
    let mut host = T::new();
    for i in 0..OBJECTS {
        host.add_rect(40.0 + 150.0 * i as f64, 60.0, 120.0, 80.0);
    }
    host.app().share.net = Some(Box::new(IrohHostNet::new(NetConfig { ready_timeout: Duration::from_secs(15), ..NetConfig::default() })));
    crate::share_ui::start(host.app());
    while host.app().share.starting.is_some() || !(host.app().share.is_running() || host.app().share.error.is_some()) {
        host.h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    let a = host.app();
    assert!(a.share.is_running(), "sharing did not start: {:?}", a.share.error);
    let link = a.share.host.as_ref().unwrap().link(Role::Edit, a.share.addr.as_ref().unwrap()).expect("a link");
    println!("LINK {link}");
    println!("LINK_READ {}", a.share.host.as_ref().unwrap().link(Role::View, a.share.addr.as_ref().unwrap()).expect("a read-only link"));
    println!("OBJECTS {}", objects(&mut host));
    let (tx, commands) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line.trim().to_owned()).is_err() {
                return;
            }
        }
        let _ = tx.send("quit".to_owned());
    });
    let mut added = 0;
    loop {
        host.h.step();
        let session = host.app().share.host.as_mut().expect("a session");
        for p in session.pending() {
            println!("MEMBER {} {:?}", p.name, p.key_role);
            session.accept(p.request, p.key_role);
        }
        match commands.try_recv().as_deref() {
            Ok("add") => {
                host.add_rect(40.0 + 150.0 * (added % 5) as f64, 220.0, 120.0, 80.0);
                added += 1;
                println!("ADDED {}", objects(&mut host));
            }
            Ok("count") => println!("COUNT {}", objects(&mut host)),
            Ok("images") => {
                let n =
                    host.app().ed.doc.pages.iter().flat_map(|p| p.objects.iter()).filter(|o| matches!(o.kind, inkboard_core::model::Kind::Image(_))).count();
                println!("IMAGES {n}");
            }
            Ok("people") => {
                let names: Vec<String> =
                    host.app().share.host.as_ref().map(|s| s.members().into_iter().filter(|m| m.connected).map(|m| m.name).collect()).unwrap_or_default();
                println!("PEOPLE {}", names.join(","));
            }
            Ok("agent") => {
                let (id, frame) = {
                    let page = host.app().ed.page();
                    let o = &page.objects[0];
                    (o.id.clone(), page.bounds(o))
                };
                let now = std::time::Instant::now();
                let event =
                    AgentEvent { name: "Claude".into(), tool: "add_shape".into(), action: Action::Act, steps: vec![vec![Target { id, frame }]], at: now };
                host.app().agent.record(event, Pt::ZERO, now);
                println!("AGENT");
            }
            Ok("quit") => break,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    crate::share_ui::stop(host.app());
}
