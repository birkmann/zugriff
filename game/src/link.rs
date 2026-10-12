//! A WebSocket to a game server, the same to use in the browser and natively: `open`, `send`, and `poll` once a
//! frame for what arrived. In the browser it is the page's `WebSocket` (kept in a thread-local, since JavaScript
//! objects can't cross threads and Bevy resources must); natively a thread runs a blocking socket.

use zugriff_sim::net::{self, ClientMsg, ServerMsg};

pub enum LinkEvent {
    Open,
    Msg(ServerMsg),
    /// Closed, with why (empty: closed normally).
    Closed(String),
}

/// A server address as typed (`host`, `host:port`, `ws://…`) made into a WebSocket URL.
pub fn normalize(addr: &str) -> String {
    let a = addr.trim().trim_end_matches('/');
    let (scheme, rest) = match a.split_once("://") {
        Some((s, r)) => (if s.eq_ignore_ascii_case("wss") || s.eq_ignore_ascii_case("https") { "wss" } else { "ws" }, r),
        None => ("ws", a),
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, net::PATH),
    };
    // A port unless the host has one (or is an IPv6 literal with one), and none for wss behind a proxy.
    let has_port = host.rsplit_once(':').is_some_and(|(h, p)| !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) && (!h.contains(':') || h.ends_with(']')));
    let host = if has_port || scheme == "wss" { host.to_string() } else { format!("{host}:{}", net::DEFAULT_PORT) };
    format!("{scheme}://{host}{path}")
}

/// A short form for lists: host and port without the scheme and the default path.
pub fn short(url: &str) -> String {
    let s = url.split_once("://").map_or(url, |(_, r)| r);
    s.strip_suffix(net::PATH).unwrap_or(s).to_string()
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::LinkEvent;
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
    use std::sync::Mutex;
    use std::time::Duration;
    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::Message;

    pub struct Link {
        tx: Sender<Vec<u8>>,
        rx: Mutex<Receiver<LinkEvent>>,
    }

    impl Link {
        pub fn open(url: &str) -> Link {
            let (tx, out_rx) = mpsc::channel::<Vec<u8>>();
            let (in_tx, rx) = mpsc::channel();
            let url = url.to_string();
            std::thread::Builder::new()
                .name("zugriff-net".into())
                .spawn(move || run(url, out_rx, in_tx))
                .expect("net thread");
            Link { tx, rx: Mutex::new(rx) }
        }

        pub fn send_bytes(&self, b: Vec<u8>) {
            let _ = self.tx.send(b);
        }

        pub fn poll(&self) -> Vec<LinkEvent> {
            let rx = self.rx.lock().unwrap();
            let mut out = Vec::new();
            loop {
                match rx.try_recv() {
                    Ok(e) => out.push(e),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            out
        }
    }

    fn run(url: String, out: Receiver<Vec<u8>>, events: Sender<LinkEvent>) {
        let mut ws = match connect(&url) {
            Ok(ws) => ws,
            Err(e) => {
                let _ = events.send(LinkEvent::Closed(e));
                return;
            }
        };
        if events.send(LinkEvent::Open).is_err() {
            return;
        }
        loop {
            // Outgoing first, then a short wait for anything incoming.
            loop {
                match out.try_recv() {
                    Ok(b) => {
                        if let Err(e) = ws.send(Message::Binary(b.into())) {
                            let _ = events.send(LinkEvent::Closed(e.to_string()));
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    // The game dropped the link.
                    Err(TryRecvError::Disconnected) => {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        return;
                    }
                }
            }
            match ws.read() {
                Ok(Message::Binary(b)) => match zugriff_sim::net::decode(&b) {
                    Some(m) => {
                        if events.send(LinkEvent::Msg(m)).is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = events.send(LinkEvent::Closed("Unverständliche Antwort vom Server (andere Version?)".into()));
                        return;
                    }
                },
                Ok(Message::Close(_)) => {
                    let _ = events.send(LinkEvent::Closed(String::new()));
                    return;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => {
                    let _ = events.send(LinkEvent::Closed(e.to_string()));
                    return;
                }
            }
        }
    }

    type Socket = tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>;

    fn connect(url: &str) -> Result<Socket, String> {
        let req = url.to_string();
        let host = url.split_once("://").map_or(url, |(_, r)| r).split('/').next().unwrap_or("").to_string();
        let addrs: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&(if host.contains(':') && !host.ends_with(']') { host.clone() } else { format!("{host}:443") }))
            .map_err(|e| format!("{host}: {e}"))?
            .collect();
        let addr = addrs.first().ok_or_else(|| format!("{host}: keine Adresse"))?;
        let tcp = std::net::TcpStream::connect_timeout(addr, Duration::from_secs(4)).map_err(|e| format!("{host}: {e}"))?;
        let _ = tcp.set_nodelay(true);
        let (mut ws, _) = tungstenite::client_tls(req, tcp).map_err(|e| e.to_string())?;
        // Short reads so outgoing commands never wait long behind an idle socket.
        match ws.get_mut() {
            MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_millis(2)));
            }
            MaybeTlsStream::Rustls(s) => {
                let _ = s.sock.set_read_timeout(Some(Duration::from_millis(2)));
            }
            _ => {}
        }
        Ok(ws)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::LinkEvent;
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

    struct Socket {
        ws: Option<WebSocket>,
        inbox: std::rc::Rc<RefCell<VecDeque<LinkEvent>>>,
        /// Messages sent before the socket opened.
        queued: std::rc::Rc<RefCell<Vec<Vec<u8>>>>,
        _handlers: Vec<Box<dyn std::any::Any>>,
    }

    thread_local! {
        static SOCKETS: RefCell<HashMap<u32, Socket>> = RefCell::new(HashMap::new());
        static NEXT: RefCell<u32> = const { RefCell::new(1) };
    }

    /// A handle to a socket in the page's thread (the only one in the browser).
    pub struct Link {
        id: u32,
    }

    impl Drop for Link {
        fn drop(&mut self) {
            SOCKETS.with(|s| {
                if let Some(ws) = s.borrow_mut().remove(&self.id).and_then(|sock| sock.ws) {
                    ws.set_onmessage(None);
                    ws.set_onclose(None);
                    ws.set_onopen(None);
                    ws.set_onerror(None);
                    let _ = ws.close();
                }
            });
        }
    }

    impl Link {
        pub fn open(url: &str) -> Link {
            let id = NEXT.with(|n| {
                let mut n = n.borrow_mut();
                *n += 1;
                *n
            });
            let inbox = std::rc::Rc::new(RefCell::new(VecDeque::new()));
            let queued = std::rc::Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
            let ws = match WebSocket::new(url) {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = e;
                    inbox.borrow_mut().push_back(LinkEvent::Closed("Ungültige Serveradresse".into()));
                    SOCKETS.with(|s| s.borrow_mut().insert(id, Socket { ws: None, inbox, queued, _handlers: Vec::new() }));
                    return Link { id };
                }
            };
            ws.set_binary_type(BinaryType::Arraybuffer);
            let on_msg = {
                let inbox = inbox.clone();
                Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                    if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                        let ev = match zugriff_sim::net::decode(&bytes) {
                            Some(m) => LinkEvent::Msg(m),
                            None => LinkEvent::Closed("Unverständliche Antwort vom Server (andere Version?)".into()),
                        };
                        inbox.borrow_mut().push_back(ev);
                    }
                })
            };
            let on_open = {
                let inbox = inbox.clone();
                let queued = queued.clone();
                let ws2 = ws.clone();
                Closure::<dyn FnMut(JsValue)>::new(move |_| {
                    for b in queued.borrow_mut().drain(..) {
                        let _ = ws2.send_with_u8_array(&b);
                    }
                    inbox.borrow_mut().push_back(LinkEvent::Open);
                })
            };
            let on_close = {
                let inbox = inbox.clone();
                Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
                    let why = if e.was_clean() { String::new() } else { format!("Verbindung getrennt ({})", e.code()) };
                    inbox.borrow_mut().push_back(LinkEvent::Closed(why));
                })
            };
            let on_error = Closure::<dyn FnMut(JsValue)>::new(move |_| {});
            ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
            ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
            ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            let handlers: Vec<Box<dyn std::any::Any>> = vec![Box::new(on_msg), Box::new(on_open), Box::new(on_close), Box::new(on_error)];
            SOCKETS.with(|s| s.borrow_mut().insert(id, Socket { ws: Some(ws), inbox, queued, _handlers: handlers }));
            Link { id }
        }

        pub fn send_bytes(&self, b: Vec<u8>) {
            SOCKETS.with(|s| {
                if let Some(sock) = s.borrow().get(&self.id)
                    && let Some(ws) = &sock.ws
                {
                    if ws.ready_state() == WebSocket::OPEN {
                        let _ = ws.send_with_u8_array(&b);
                    } else if ws.ready_state() == WebSocket::CONNECTING {
                        sock.queued.borrow_mut().push(b);
                    }
                }
            });
        }

        pub fn poll(&self) -> Vec<LinkEvent> {
            SOCKETS.with(|s| s.borrow().get(&self.id).map(|sock| sock.inbox.borrow_mut().drain(..).collect()).unwrap_or_default())
        }
    }
}

pub use imp::Link;

impl Link {
    pub fn send(&self, msg: &ClientMsg) {
        self.send_bytes(net::encode(msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(normalize("localhost"), "ws://localhost:27015/zg");
        assert_eq!(normalize("example.org:4000"), "ws://example.org:4000/zg");
        assert_eq!(normalize("wss://game.example.org"), "wss://game.example.org/zg");
        assert_eq!(normalize("ws://1.2.3.4:27015/zg"), "ws://1.2.3.4:27015/zg");
        assert_eq!(short("ws://1.2.3.4:27015/zg"), "1.2.3.4:27015");
    }
}
