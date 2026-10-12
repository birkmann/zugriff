//! One client's connection: the hello, the room list, creating and joining rooms; once in a room, its messages go
//! to the room and the room writes straight to the socket.

use crate::hub::{Hub, RoomHandle};
use crate::room::{JoinReq, RoomIn};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use zugriff_sim::net::{self, ClientMsg, RoomRef, ServerMsg, PROTOCOL};

/// Messages queued for a client beyond this mean it can't keep up: it is dropped (snapshots are deltas, so none can
/// be skipped).
pub const OUT_QUEUE: usize = 600;

pub type Out = mpsc::Sender<Vec<u8>>;

pub fn send(out: &Out, msg: &ServerMsg) -> bool {
    out.try_send(net::encode(msg)).is_ok()
}

pub async fn run(stream: TcpStream, peer: SocketAddr, hub: Arc<Hub>) {
    let cfg = WebSocketConfig::default().max_message_size(Some(256 << 10)).max_frame_size(Some(256 << 10));
    let Ok(ws) = tokio_tungstenite::accept_async_with_config(stream, Some(cfg)).await else { return };
    let (mut sink, mut stream) = ws.split();
    let (out, mut rx) = mpsc::channel::<Vec<u8>>(OUT_QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(b) = rx.recv().await {
            if sink.send(Message::Binary(b.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let id = hub.client_id();
    let mut name = String::from("Spieler");
    let mut finishes: Vec<u8> = Vec::new();
    let mut hello = false;
    let mut room: Option<RoomHandle> = None;
    while let Some(Ok(frame)) = stream.next().await {
        let bytes = match frame {
            Message::Binary(b) => b,
            Message::Close(_) => break,
            _ => continue,
        };
        let Some(msg) = net::decode::<ClientMsg>(&bytes) else {
            send(&out, &ServerMsg::Error("Unverständliche Nachricht.".into()));
            continue;
        };
        if out.is_closed() {
            break;
        }
        match msg {
            ClientMsg::Hello { protocol, name: n, finishes: f } => {
                finishes = f.into_iter().take(64).collect();
                if protocol != PROTOCOL {
                    send(&out, &ServerMsg::Error(format!("Andere Spielversion (Server {PROTOCOL}, Spiel {protocol}). Bitte die Seite neu laden.")));
                    break;
                }
                hello = true;
                name = net::clean_name(&n, "Spieler");
                send(&out, &ServerMsg::Welcome { server: hub.name.clone(), motd: hub.motd.clone() });
            }
            ClientMsg::List => {
                send(&out, &ServerMsg::Rooms(hub.list(true)));
            }
            ClientMsg::Ping(t) => {
                send(&out, &ServerMsg::Pong(t));
            }
            ClientMsg::Announce { address, name, rooms } => {
                hub.announce(address, name, rooms);
            }
            ClientMsg::Create(setup) if hello => {
                if !hub.allow_create {
                    send(&out, &ServerMsg::Error("Auf diesem Server können keine Räume erstellt werden.".into()));
                    continue;
                }
                let mut setup = setup;
                setup.name = net::clean_name(&setup.name, &format!("{name}s Raum"));
                setup.per_side = setup.per_side.clamp(1, 8);
                setup.difficulty = setup.difficulty.min(3);
                setup.max_rounds = setup.max_rounds.clamp(2, 30);
                setup.password = setup.password.chars().take(32).collect();
                leave(&mut room, id);
                match hub.create(setup, Vec::new(), false).await {
                    Ok((_, handle)) => {
                        if join(&handle, id, &name, &finishes, None, true, &out).await {
                            room = Some(handle);
                        }
                    }
                    Err(e) => {
                        send(&out, &ServerMsg::Error(e));
                    }
                }
            }
            ClientMsg::Join { room: which, password, team } if hello => {
                let Some(handle) = hub.find(&which) else {
                    let what = if matches!(which, RoomRef::Code(_)) { "Kein Raum mit diesem Code." } else { "Diesen Raum gibt es nicht mehr." };
                    send(&out, &ServerMsg::Error(what.into()));
                    continue;
                };
                if !handle.password.is_empty() && handle.password != password {
                    send(&out, &ServerMsg::Error(if password.is_empty() { "Passwort nötig.".into() } else { "Falsches Passwort.".into() }));
                    continue;
                }
                if room.as_ref().is_some_and(|r| r.info.lock().unwrap().id == handle.info.lock().unwrap().id) {
                    continue;
                }
                leave(&mut room, id);
                if join(&handle, id, &name, &finishes, team, false, &out).await {
                    room = Some(handle);
                }
            }
            ClientMsg::Leave => leave(&mut room, id),
            other => {
                if let Some(r) = &room
                    && r.tx.send(RoomIn::Msg(id, other)).is_err()
                {
                    room = None;
                }
            }
        }
    }
    leave(&mut room, id);
    drop(out);
    let _ = writer.await;
    let _ = peer;
}

async fn join(handle: &RoomHandle, id: u64, name: &str, finishes: &[u8], team: Option<zugriff_sim::game::Team>, owner: bool, out: &Out) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let req = JoinReq { client: id, name: name.to_string(), finishes: finishes.to_vec(), team, owner, out: out.clone(), reply: tx };
    if handle.tx.send(RoomIn::Join(Box::new(req))).is_err() {
        send(out, &ServerMsg::Error("Der Raum wurde gerade geschlossen.".into()));
        return false;
    }
    match rx.await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            send(out, &ServerMsg::Error(e));
            false
        }
        Err(_) => false,
    }
}

fn leave(room: &mut Option<RoomHandle>, id: u64) {
    if let Some(r) = room.take() {
        let _ = r.tx.send(RoomIn::Gone(id));
    }
}
