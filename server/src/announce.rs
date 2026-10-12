//! Telling a master server about this one: every few seconds, over a WebSocket to it, this server's public rooms and
//! the address players should use. The master lists them next to its own rooms.

use crate::hub::Hub;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use zugriff_sim::net::{self, ClientMsg, PROTOCOL};

pub async fn run(hub: Arc<Hub>, master: String, public: String) {
    loop {
        match tokio_tungstenite::connect_async(master.as_str()).await {
            Ok((ws, _)) => {
                println!("announcing to {master} as {public}");
                let (mut sink, mut stream) = ws.split();
                // Answers are not needed; read them so the connection stays healthy.
                let reader = tokio::spawn(async move { while let Some(Ok(_)) = stream.next().await {} });
                let hello = net::encode(&ClientMsg::Hello { protocol: PROTOCOL, name: hub.name.clone(), finishes: Vec::new() });
                if sink.send(Message::Binary(hello.into())).await.is_ok() {
                    loop {
                        let msg = ClientMsg::Announce { address: public.clone(), name: hub.name.clone(), rooms: hub.list(false) };
                        if sink.send(Message::Binary(net::encode(&msg).into())).await.is_err() || reader.is_finished() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
                reader.abort();
                eprintln!("lost the master server {master}; retrying");
            }
            Err(e) => eprintln!("master server {master}: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
