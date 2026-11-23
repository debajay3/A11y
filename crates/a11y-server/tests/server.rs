//! The server logic in-process on `Network::Loopback`: guests join through a11y-net, edit, and
//! the document, the autosave and the link survive a restart.

use a11y_collab::wire::{self, FontKey, ToGuest, ToHost};
use a11y_collab::{ClientMsg, GuestSession, Role, ServerMsg};
use a11y_doc::{Document, Node, NodeId, Rect};
use a11y_net::{DisconnectReason, Guest, GuestEvent, GuestOptions, JoinError, Network, Ticket};
use a11y_server::{Config, Server};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const T: Duration = Duration::from_secs(20);

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("a11y-server-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn free_port() -> u16 {
    std::net::UdpSocket::bind("0.0.0.0:0").unwrap().local_addr().unwrap().port()
}

fn seed(path: &Path) -> NodeId {
    let mut d = Document::new("Shared");
    let page = d.add_page("Page 1");
    let a = d.alloc_id();
    d.insert(page, Node::rectangle(a, Rect::new(0.0, 0.0, 100.0, 80.0))).unwrap();
    std::fs::write(path, d.to_json().unwrap()).unwrap();
    a
}

fn config(file: &Path, port: u16) -> Config {
    let mut c = Config::new(file);
    c.password = Some("pw".into());
    c.network = Network::Loopback;
    c.port = Some(port);
    c.autosave = Duration::from_millis(200);
    c.link_file = Some(file.with_extension("link"));
    c
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Server>,
}

impl Running {
    fn start(mut server: Server) -> Running {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        Running {
            stop,
            thread: std::thread::spawn(move || {
                server.run(&s);
                server
            }),
        }
    }
    fn stop(self) -> Server {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.join().unwrap()
    }
}

fn join(link: &str, name: &str, pw: Option<&str>) -> Result<Guest, JoinError> {
    let ticket = Ticket::parse(link).unwrap();
    Guest::join_blocking(&ticket, name, pw, GuestOptions { network: Network::Loopback, ..Default::default() }, T).map(|(g, _)| g)
}

fn send(g: &Guest, m: ClientMsg) {
    g.send(serde_json::to_vec(&ToHost::Sync(m)).unwrap()).unwrap();
}

/// Waits for the snapshot and starts a guest session on it. Presence seen meanwhile comes along.
fn open(g: &Guest) -> (GuestSession, Document, Role, Vec<serde_json::Value>) {
    let end = Instant::now() + T;
    let mut role = Role::Viewer;
    let mut presence = Vec::new();
    loop {
        let left = end.checked_duration_since(Instant::now()).expect("no snapshot");
        match g.recv_timeout(left) {
            Some(GuestEvent::Presence(b)) => presence.push(serde_json::from_slice(&b).unwrap()),
            Some(GuestEvent::Message(b)) => match serde_json::from_slice::<ToGuest>(&b).unwrap() {
                ToGuest::Role(r) => role = r,
                ToGuest::Sync(s @ ServerMsg::Snapshot { .. }) => {
                    let (sess, doc, reqs) = GuestSession::join(s).unwrap();
                    assert!(reqs.is_empty());
                    return (sess, doc, role, presence);
                }
                ToGuest::Sync(_) | ToGuest::Images(_) | ToGuest::Font { .. } => {}
            },
            _ => {}
        }
    }
}

/// Sends the guest's edits and pumps the host's answers until they are acknowledged.
fn commit(g: &Guest, sess: &mut GuestSession, doc: &mut Document) {
    if let Some(m) = sess.local_changes(doc) {
        send(g, m);
    }
    let end = Instant::now() + T;
    while sess.has_pending() {
        let left = end.checked_duration_since(Instant::now()).expect("edit never acknowledged");
        if let Some(GuestEvent::Message(b)) = g.recv_timeout(left) {
            if let ToGuest::Sync(m) = serde_json::from_slice(&b).unwrap() {
                for out in sess.receive(doc, m).out {
                    send(g, out);
                }
            }
        }
    }
}

fn wait_for<R>(what: &str, mut f: impl FnMut() -> Option<R>) -> R {
    let end = Instant::now() + T;
    loop {
        if let Some(r) = f() {
            return r;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn names(doc: &Document) -> Vec<String> {
    let mut v: Vec<String> = doc.nodes.values().map(|n| n.name.clone()).collect();
    v.sort();
    v
}

#[test]
fn collaborator_edits_after_creator_leaves_and_creator_sees_changes_on_return() {
    let d = dir("creator-offline");
    let file = d.join("shared.a11y");
    let node = seed(&file);
    let running = Running::start(Server::start(config(&file, free_port())).unwrap());
    let link = std::fs::read_to_string(file.with_extension("link")).unwrap();
    let link = link.trim();

    let creator = join(link, "Creator", Some("pw")).unwrap();
    let _ = open(&creator);
    let collaborator = join(link, "Collaborator", Some("pw")).unwrap();
    let (mut session, mut document, role, _) = open(&collaborator);
    assert_eq!(role, Role::Editor);

    creator.leave();
    drop(creator);
    document.nodes.get_mut(&node).unwrap().name = "Edited while creator was offline".into();
    commit(&collaborator, &mut session, &mut document);
    wait_for("offline creator edit to autosave", || {
        let saved = Document::from_json(&std::fs::read_to_string(&file).ok()?).ok()?;
        (saved.get(node)?.name == "Edited while creator was offline").then_some(())
    });
    collaborator.leave();
    drop(collaborator);

    // No desktop client remains connected. A fresh connection must still see the saved edits.
    let returning_creator = join(link, "Creator", Some("pw")).unwrap();
    let (_, latest, _, _) = open(&returning_creator);
    assert_eq!(latest.get(node).unwrap().name, "Edited while creator was offline");
    returning_creator.leave();
    drop(returning_creator);
    running.stop().shutdown().unwrap();
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn edits_autosave_and_the_link_survive_a_restart() {
    let d = dir("restart");
    let file = d.join("shared.a11y");
    let a = seed(&file);
    let port = free_port();

    // --- first run
    let server = Server::start(config(&file, port)).unwrap();
    let link1 = server.link().to_string();
    assert!(link1.starts_with("a11y://join/"));
    assert_eq!(std::fs::read_to_string(file.with_extension("link")).unwrap().trim(), link1);
    let key_file = d.join(".shared.a11y-key");
    assert!(key_file.exists(), "key file next to the document");
    let running = Running::start(server);

    assert_eq!(join(&link1, "Eve", Some("wrong")).err(), Some(JoinError::WrongPassword));
    assert_eq!(join(&link1, "Eve", None).err(), Some(JoinError::WrongPassword));

    let ana = join(&link1, "Ana", Some("pw")).unwrap();
    let (mut sess, mut doc, role, _) = open(&ana);
    assert_eq!(role, Role::Editor);
    let page = doc.pages[0];
    doc.nodes.get_mut(&a).unwrap().name = "Renamed by Ana".into();
    let b = doc.alloc_id();
    assert_eq!(b.0 >> 40, u64::from(sess.peer()), "guest ids come from its own range");
    let mut rect = Node::rectangle(b, Rect::new(10.0, 10.0, 20.0, 20.0));
    rect.name = "Added by Ana".into();
    doc.insert(page, rect).unwrap();
    commit(&ana, &mut sess, &mut doc);

    // Presence goes out stamped with the sender, is replayed to a newcomer, and "gone" follows a leave.
    let hello = serde_json::json!({ "peer": 99, "name": "Ana", "page": page.0, "cursor": [1.0, 2.0], "sel": [], "view": [0.0, 0.0, 1.0] });
    ana.send_presence(hello.to_string().as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let bea = join(&link1, "Bea", Some("pw")).unwrap();
    let (_, bdoc, _, mut early) = open(&bea);
    let ana_peer = Some(u64::from(sess.peer()));
    // The replay follows the snapshot, so it may still be on its way.
    let p = wait_for("Ana's presence to be replayed", || {
        if let Some(GuestEvent::Presence(b)) = bea.recv_timeout(Duration::from_millis(50)) {
            early.push(serde_json::from_slice(&b).unwrap());
        }
        early.iter().find(|p| p["peer"].as_u64() == ana_peer).cloned()
    });
    assert_eq!(p["name"].as_str(), Some("Ana"));
    assert!(!p["gone"].as_bool().unwrap_or(false));
    // Bea's snapshot already has Ana's edits.
    assert!(names(&bdoc).contains(&"Added by Ana".to_string()));
    ana.leave();
    wait_for("gone", || match bea.recv_timeout(Duration::from_millis(100)) {
        Some(GuestEvent::Presence(b)) => {
            let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
            (v["gone"].as_bool() == Some(true) && v["peer"].as_u64() == ana_peer).then_some(())
        }
        _ => None,
    });
    drop(bea);

    // The autosave writes the edits without anybody saving.
    wait_for("autosave", || {
        let doc = Document::from_json(&std::fs::read_to_string(&file).ok()?).ok()?;
        names(&doc).contains(&"Added by Ana".to_string()).then_some(())
    });
    let server = running.stop();
    assert!(names(server.document()).contains(&"Renamed by Ana".to_string()));
    server.shutdown().unwrap();
    assert!(!d.join("shared.a11y.tmp").exists(), "temp file is renamed away");
    let saved = Document::from_json(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved.get(a).unwrap().name, "Renamed by Ana");
    assert!(d.join("shared.a11y.bak.1").exists(), "previous version kept as a backup");

    // --- second run: same key file, same port
    let server = Server::start(config(&file, port)).unwrap();
    assert_eq!(server.link(), link1, "the link is stable across restarts");
    assert_eq!(names(server.document()), names(&saved));
    let running = Running::start(server);
    let cy = join(&link1, "Cy", Some("pw")).unwrap();
    let (_, cdoc, ..) = open(&cy);
    assert_eq!(cdoc.get(a).unwrap().name, "Renamed by Ana");
    assert!(names(&cdoc).contains(&"Added by Ana".to_string()));

    // Stopping the server tells guests the host ended.
    let server = running.stop();
    server.shutdown().unwrap();
    wait_for("end of session", || match cy.recv_timeout(Duration::from_millis(100)) {
        Some(GuestEvent::Disconnected(DisconnectReason::HostEnded)) => Some(()),
        _ => None,
    });
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn view_only_guests_cannot_change_the_document() {
    let d = dir("viewonly");
    let file = d.join("v.a11y");
    let a = seed(&file);
    let mut cfg = config(&file, free_port());
    cfg.view_only = true;
    cfg.password = None;
    let server = Server::start(cfg).unwrap();
    let link = server.link().to_string();
    let running = Running::start(server);
    let g = join(&link, "Vi", None).unwrap();
    let (mut sess, mut doc, role, _) = open(&g);
    assert_eq!(role, Role::Viewer);
    doc.nodes.get_mut(&a).unwrap().name = "Nope".into();
    commit(&g, &mut sess, &mut doc);
    let server = running.stop();
    assert_ne!(server.document().get(a).unwrap().name, "Nope");
    server.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_figma_import_is_saved_as_a11y_and_then_wins() {
    let d = dir("import");
    let mut doc = Document::new("From json");
    doc.add_page("Page 1");
    // A native document in a .json file is accepted by the importer too.
    std::fs::write(d.join("in.json"), doc.to_json().unwrap()).unwrap();
    let mut cfg = config(&d.join("in.json"), free_port());
    cfg.password = None;
    let server = Server::start(cfg.clone()).unwrap();
    assert_eq!(server.path(), d.join("in.a11y"));
    assert!(d.join("in.a11y").exists());
    server.shutdown().unwrap();
    // Restarting with the same command serves the saved .a11y, not the source again.
    std::fs::remove_file(d.join("in.json")).unwrap();
    let server = Server::start(cfg).unwrap();
    assert_eq!(server.path(), d.join("in.a11y"));
    server.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(d);
}

/// Next reliable message from the host, as a typed value.
fn next_msg(g: &Guest) -> ToGuest {
    let end = Instant::now() + T;
    loop {
        let left = end.checked_duration_since(Instant::now()).expect("no message");
        if let Some(GuestEvent::Message(b)) = g.recv_timeout(left) {
            return serde_json::from_slice(&b).unwrap();
        }
    }
}

#[test]
fn image_digests_arrive_before_the_snapshot_and_fonts_get_an_empty_answer() {
    let d = dir("images");
    let file = d.join("i.a11y");
    let mut doc = Document::new("Shared");
    doc.add_page("Page 1");
    doc.images.insert("img-1".into(), vec![1, 2, 3, 4]);
    std::fs::write(&file, doc.to_json().unwrap()).unwrap();
    let mut cfg = config(&file, free_port());
    cfg.password = None;
    let server = Server::start(cfg).unwrap();
    let link = server.link().to_string();
    let running = Running::start(server);
    let g = join(&link, "Ivy", None).unwrap();

    assert!(matches!(next_msg(&g), ToGuest::Role(Role::Editor)));
    match next_msg(&g) {
        ToGuest::Images(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!((list[0].key.as_str(), list[0].digest.as_str()), ("img-1", wire::digest(&[1, 2, 3, 4]).as_str()));
        }
        _ => panic!("the image digests come right after the role"),
    }
    assert!(matches!(next_msg(&g), ToGuest::Sync(ServerMsg::Snapshot { .. })));
    assert!(matches!(next_msg(&g), ToGuest::Sync(ServerMsg::Resume { .. })));

    let key = FontKey { family: "Inter".into(), weight: 400, italic: false };
    g.send(serde_json::to_vec(&ToHost::Fonts(vec![key.clone()])).unwrap()).unwrap();
    match next_msg(&g) {
        ToGuest::Font { key: k, bytes } => {
            assert_eq!(k, key);
            assert!(bytes.is_empty(), "the server has no fonts to share");
        }
        _ => panic!("expected the font answer"),
    }
    running.stop().shutdown().unwrap();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_severed_guest_resumes_as_the_same_peer_and_edits_converge() {
    let d = dir("resume");
    let file = d.join("r.a11y");
    let a = seed(&file);
    let mut cfg = config(&file, free_port());
    cfg.password = None;
    let server = Server::start(cfg).unwrap();
    let link = server.link().to_string();
    let sever = server.sever_flag();
    let running = Running::start(server);

    let ana = join(&link, "Ana", None).unwrap();
    let (mut sess, mut doc, _, _) = open(&ana);
    let peer = sess.peer();
    // the secret follows the snapshot
    let mut token = None;
    while token.is_none() {
        if let ToGuest::Sync(m) = next_msg(&ana) {
            sess.receive(&mut doc, m);
        }
        token = sess.token().map(str::to_string);
    }

    // the network fails; Ana edits offline while Bea joins and edits
    sever.store(true, Ordering::Relaxed);
    wait_for("the loss", || match ana.recv_timeout(Duration::from_millis(100)) {
        Some(GuestEvent::Disconnected(DisconnectReason::Lost(_))) => Some(()),
        _ => None,
    });
    doc.nodes.get_mut(&a).unwrap().name = "Ana offline".into();
    let c = doc.alloc_id();
    let mut rect = Node::rectangle(c, Rect::new(5.0, 5.0, 10.0, 10.0));
    rect.name = "Added offline".into();
    let page = doc.pages[0];
    doc.insert(page, rect).unwrap();
    let bea = join(&link, "Bea", None).unwrap();
    let (mut bsess, mut bdoc, ..) = open(&bea);
    assert_ne!(bsess.peer(), peer);
    let bid = bdoc.alloc_id();
    let mut brect = Node::rectangle(bid, Rect::new(1.0, 1.0, 5.0, 5.0));
    brect.name = "Added by Bea".into();
    let bpage = bdoc.pages[0];
    bdoc.insert(bpage, brect).unwrap();
    commit(&bea, &mut bsess, &mut bdoc);

    // Ana returns with her secret: same peer index, no password needed to be asked again
    let ticket = Ticket::parse(&link).unwrap();
    let ana2 = Guest::join_resume(&ticket, "Ana", None, GuestOptions { network: Network::Loopback, ..Default::default() }, token).unwrap();
    ana2.wait_joined(T).unwrap();
    let snapshot = loop {
        if let ToGuest::Sync(s @ ServerMsg::Snapshot { .. }) = next_msg(&ana2) {
            break s;
        }
    };
    assert!(matches!(snapshot, ServerMsg::Snapshot { peer: p, .. } if p == peer), "same peer index");
    let r = sess.rejoin(&mut doc, snapshot).expect("snapshot for the same peer");
    for m in r.out {
        send(&ana2, m);
    }
    commit(&ana2, &mut sess, &mut doc);
    assert!(names(&doc).contains(&"Added by Bea".to_string()), "the outage's edits arrive with the snapshot");

    let server = running.stop();
    let on_server = server.document();
    assert_eq!(on_server.get(a).unwrap().name, "Ana offline");
    assert!(names(on_server).contains(&"Added offline".to_string()));
    assert!(names(on_server).contains(&"Added by Bea".to_string()));
    assert_eq!(on_server.nodes.get(&c).map(|n| n.name.as_str()), Some("Added offline"));
    assert_eq!(c.0 >> 40, u64::from(peer));
    server.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn managed_sessions_require_live_access_and_revoke_connections() {
    let d = dir("managed-auth");
    let path = d.join("shared.a11y");
    seed(&path);
    let allowed = Arc::new(AtomicBool::new(true));
    let check = allowed.clone();
    let mut cfg = config(&path, free_port());
    cfg.password = None;
    cfg.access_check =
        Some(a11y_server::AccessCheck(Arc::new(
            move |token| {
                if check.load(Ordering::Relaxed) && token == "valid-session" {
                    Some(Role::Viewer)
                } else {
                    None
                }
            },
        )));
    let server = Server::start(cfg).unwrap();
    let link = server.link().to_string();
    let running = Running::start(server);
    assert!(join(&link, "Anonymous", None).is_err());
    assert!(join(&link, "Old key", Some("old-workspace-key")).is_err());
    let guest = join(&link, "Viewer", Some("valid-session")).unwrap();
    let (_, _, role, _) = open(&guest);
    assert_eq!(role, Role::Viewer);
    allowed.store(false, Ordering::Relaxed);
    let until = Instant::now() + T;
    let mut disconnected = false;
    while Instant::now() < until {
        if matches!(guest.recv_timeout(Duration::from_millis(100)), Some(GuestEvent::Disconnected(_))) {
            disconnected = true;
            break;
        }
    }
    assert!(disconnected, "revoked user must be disconnected");
    assert!(join(&link, "Revoked", Some("valid-session")).is_err());
    drop(guest);
    running.stop().shutdown().unwrap();
    let _ = std::fs::remove_dir_all(d);
}
