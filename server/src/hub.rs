//! The server's rooms, the maps they share, and the servers announced to this one (as a master).

use crate::room::{self, RoomIn};
use crate::Args;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use zugriff_sim::compile::Level;
use zugriff_sim::nav::{self, Nav};
use zugriff_sim::net::{RoomInfo, RoomSetup};

/// A running room, as others reach it.
#[derive(Clone)]
pub struct RoomHandle {
    pub tx: UnboundedSender<RoomIn>,
    pub info: Arc<Mutex<RoomInfo>>,
    pub code: String,
    pub password: String,
}

/// Another server's rooms, as it announced them.
struct Announced {
    at: Instant,
    rooms: Vec<RoomInfo>,
}

pub struct Hub {
    pub name: String,
    pub motd: String,
    pub assets: PathBuf,
    pub allow_create: bool,
    pub max_rooms: usize,
    rooms: Mutex<HashMap<u32, RoomHandle>>,
    announced: Mutex<HashMap<String, Announced>>,
    maps: Mutex<HashMap<String, (Arc<Level>, Arc<Nav>)>>,
    next_room: AtomicU32,
    next_client: AtomicU64,
    started: Instant,
}

pub type Map = (Arc<Level>, Arc<Nav>);

impl Hub {
    pub fn new(args: &Args) -> Arc<Hub> {
        Arc::new(Hub {
            name: args.name.clone(),
            motd: args.motd.clone(),
            assets: args.assets.clone(),
            allow_create: !args.no_create,
            max_rooms: args.max_rooms,
            rooms: Mutex::new(HashMap::new()),
            announced: Mutex::new(HashMap::new()),
            maps: Mutex::new(HashMap::new()),
            next_room: AtomicU32::new(1),
            next_client: AtomicU64::new(1),
            started: Instant::now(),
        })
    }

    /// Seconds since the server started (the ping clock).
    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn client_id(&self) -> u64 {
        self.next_client.fetch_add(1, Ordering::Relaxed)
    }

    /// The maps in the assets folder.
    pub fn maps_available(&self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.assets.join("maps"))
            .map(|d| d.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".map")).map(str::to_string)).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    /// A map, compiled with its navigation (cached: rooms on the same map share it). Blocking; call off the runtime.
    pub fn load_map(&self, name: &str) -> Result<Map, String> {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
            return Err(format!("unbekannte Karte „{name}“"));
        }
        if let Some(m) = self.maps.lock().unwrap().get(name) {
            return Ok(m.clone());
        }
        let dir = self.assets.join("maps");
        let src = std::fs::read_to_string(dir.join(format!("{name}.map"))).map_err(|_| format!("unbekannte Karte „{name}“"))?;
        let parsed = zugriff_sim::map::parse(&src).map_err(|e| format!("{name}: {e}"))?;
        let level = zugriff_sim::compile::compile(&parsed);
        let want = nav::geometry_hash(&level.world);
        let nav = std::fs::read(dir.join(format!("{name}.nav"))).ok().and_then(|b| Nav::from_bytes(&b)).filter(|n| n.hash == want).unwrap_or_else(|| {
            eprintln!("{name}: no current .nav, building the navigation");
            Nav::build(&level, &zugriff_sim::movement::MoveParams::default())
        });
        let m = (Arc::new(level), Arc::new(nav));
        self.maps.lock().unwrap().insert(name.to_string(), m.clone());
        Ok(m)
    }

    /// Starts a room; returns its handle.
    pub async fn create(self: &Arc<Self>, setup: RoomSetup, rotation: Vec<String>, persistent: bool) -> Result<(u32, RoomHandle), String> {
        if self.rooms.lock().unwrap().len() >= self.max_rooms {
            return Err("Auf diesem Server laufen schon zu viele Räume.".into());
        }
        let hub = self.clone();
        let name = setup.map.clone();
        let map = tokio::task::spawn_blocking(move || hub.load_map(&name)).await.map_err(|e| e.to_string())??;
        let id = self.next_room.fetch_add(1, Ordering::Relaxed);
        let code = if setup.private {
            zugriff_sim::net::room_code(id as u64 * 0x9e37_79b9 ^ self.started.elapsed().as_nanos() as u64 ^ 0x5bd1_e995)
        } else {
            id.to_string()
        };
        let handle = room::spawn(self.clone(), id, code.clone(), setup, map, rotation, persistent);
        self.rooms.lock().unwrap().insert(id, handle.clone());
        Ok((id, handle))
    }

    /// A room from `--room map[,map…][:name]`.
    pub async fn create_persistent(self: &Arc<Self>, spec: &str) -> Result<u32, String> {
        let (maps, name) = spec.split_once(':').unwrap_or((spec, ""));
        let rotation: Vec<String> = maps.split(',').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect();
        let first = rotation.first().cloned().ok_or("keine Karte")?;
        let name = if name.is_empty() { format!("{} · {}", self.name, first) } else { name.to_string() };
        let setup = RoomSetup { name, map: first, ..Default::default() };
        self.create(setup, rotation, true).await.map(|(id, _)| id)
    }

    pub fn remove(&self, id: u32) {
        self.rooms.lock().unwrap().remove(&id);
    }

    /// A room by id (public rooms) or by code (private ones: only the code finds them).
    pub fn find(&self, room: &zugriff_sim::net::RoomRef) -> Option<RoomHandle> {
        let rooms = self.rooms.lock().unwrap();
        match room {
            zugriff_sim::net::RoomRef::Id(id) => rooms.get(id).filter(|h| !h.info.lock().unwrap().private).cloned(),
            zugriff_sim::net::RoomRef::Code(code) => {
                let code = code.trim().to_uppercase();
                rooms.values().find(|h| h.code == code).cloned()
            }
        }
    }

    /// The public rooms, this server's first, then (with `others`) those other servers announced.
    pub fn list(&self, others: bool) -> Vec<RoomInfo> {
        let mut out: Vec<RoomInfo> = self.rooms.lock().unwrap().values().map(|h| h.info.lock().unwrap().clone()).filter(|r| !r.private).collect();
        out.sort_by_key(|r| r.id);
        if others {
            let mut ann = self.announced.lock().unwrap();
            ann.retain(|_, a| a.at.elapsed() < Duration::from_secs(20));
            for a in ann.values() {
                out.extend(a.rooms.iter().cloned());
            }
        }
        out
    }

    /// Another server's announcement (its rooms carry its address).
    pub fn announce(&self, address: String, name: String, rooms: Vec<RoomInfo>) {
        let rooms = rooms
            .into_iter()
            .filter(|r| !r.private)
            .take(64)
            .map(|mut r| {
                r.server = name.chars().take(40).collect();
                r.address = Some(address.clone());
                r
            })
            .collect();
        let mut ann = self.announced.lock().unwrap();
        // A master lists at most 64 other servers (the oldest silent ones make room).
        ann.retain(|_, a| a.at.elapsed() < Duration::from_secs(20));
        if ann.len() >= 64 && !ann.contains_key(&address) {
            return;
        }
        ann.insert(address, Announced { at: Instant::now(), rooms });
    }
}
