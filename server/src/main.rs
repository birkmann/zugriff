//! ZUGRIFF's dedicated server. One process hosts many rooms (a room is one match); players browse them, create public
//! or private ones and join over a WebSocket (`ws://host:27015/zg`). The protocol is `zugriff_sim::net`.
//!
//! ```sh
//! cargo run --release -p zugriff_server -- --name "Mein Server" --room zg_hafen,zg_altstadt:Dauerbetrieb
//! ```
//!
//! Options:
//! - `--port <n>` (27015), `--name <text>`, `--motd <text>`, `--assets <dir>` (assets).
//! - `--room <map[,map…]>[:<name>]` (repeatable): a public room that is always there, cycling its maps.
//! - `--no-create`: players can't create rooms here (only the `--room` ones exist).
//! - `--max-rooms <n>` (32).
//! - `--master <ws url>` and `--public <ws url>`: announce this server's public rooms to a master server under the
//!   address players should connect to, so they show in its list.
//!
//! Plain HTTP on the same port: `GET /rooms` gives the list as JSON (with announced servers), `GET /` a status line.

mod announce;
mod conn;
mod hub;
mod room;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zugriff_sim::net::DEFAULT_PORT;

pub struct Args {
    pub port: u16,
    pub name: String,
    pub motd: String,
    pub assets: PathBuf,
    pub rooms: Vec<String>,
    pub no_create: bool,
    pub max_rooms: usize,
    pub master: Option<String>,
    pub public: Option<String>,
}

fn args() -> Args {
    let argv: Vec<String> = std::env::args().collect();
    let get = |name: &str| argv.iter().position(|a| a == name).and_then(|i| argv.get(i + 1).cloned());
    let all = |name: &str| argv.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| argv.get(i + 1).cloned()).collect::<Vec<_>>();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").trim_start()).collect::<Vec<_>>().join("\n"));
        std::process::exit(0);
    }
    Args {
        port: get("--port").and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT),
        name: get("--name").unwrap_or_else(|| "ZUGRIFF-Server".into()),
        motd: get("--motd").unwrap_or_default(),
        assets: zugriff_sim::assets::locate(get("--assets").map(PathBuf::from)).unwrap_or_else(|error| { eprintln!("{error}"); std::process::exit(1) }),
        rooms: all("--room"),
        no_create: argv.iter().any(|a| a == "--no-create"),
        max_rooms: get("--max-rooms").and_then(|p| p.parse().ok()).unwrap_or(32),
        master: get("--master"),
        public: get("--public"),
    }
}

#[tokio::main]
async fn main() {
    let args = args();
    let hub = hub::Hub::new(&args);
    for spec in &args.rooms {
        match hub.create_persistent(spec).await {
            Ok(id) => println!("room {id}: {spec}"),
            Err(e) => eprintln!("room {spec}: {e}"),
        }
    }
    if let Some(master) = args.master.clone() {
        let public = args.public.clone().unwrap_or_else(|| format!("ws://127.0.0.1:{}{}", args.port, zugriff_sim::net::PATH));
        tokio::spawn(announce::run(hub.clone(), master, public));
    }
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("can't listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("{} listening on ws://{addr}{} (maps: {})", args.name, zugriff_sim::net::PATH, hub.maps_available().join(", "));
    loop {
        let Ok((stream, peer)) = listener.accept().await else { continue };
        let _ = stream.set_nodelay(true);
        let hub = hub.clone();
        tokio::spawn(async move {
            if is_websocket(&stream).await {
                conn::run(stream, peer, hub).await;
            } else {
                http(stream, &hub).await;
            }
        });
    }
}

/// Whether the request waiting on the socket is a WebSocket upgrade (peeked, so the handshake still sees it).
async fn is_websocket(stream: &TcpStream) -> bool {
    let mut buf = [0u8; 2048];
    for _ in 0..50 {
        let Ok(n) = stream.peek(&mut buf).await else { return false };
        let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
        if head.contains("\r\n\r\n") || n == buf.len() {
            return head.contains("upgrade: websocket");
        }
        if n == 0 {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// The plain HTTP side: the room list as JSON, or a status line.
async fn http(mut stream: TcpStream, hub: &Arc<hub::Hub>) {
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let (status, kind, body) = if path.starts_with("/rooms") {
        ("200 OK", "application/json", serde_json::to_string(&hub.list(true)).unwrap_or_else(|_| "[]".into()))
    } else if path == "/" || path.starts_with("/health") {
        ("200 OK", "text/plain; charset=utf-8", format!("{}: {} rooms\n", hub.name, hub.list(false).len()))
    } else {
        ("404 Not Found", "text/plain", "not found\n".into())
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
}
