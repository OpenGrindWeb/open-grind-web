//! Replacement for Tauri's `app.emit(...)`: events are broadcast to every
//! connected browser over the `/events` WebSocket, as
//! `{ "event": "<name>", "payload": <json> }`. The front shim turns them back
//! into `listen(name, handler)` callbacks.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
	pub event: String,
	pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct EventHub {
	tx: broadcast::Sender<Event>,
}

impl EventHub {
	pub fn new() -> Self {
		let (tx, _) = broadcast::channel(1024);
		Self { tx }
	}

	pub fn emit(&self, event: &str, payload: impl Serialize) {
		let payload = match serde_json::to_value(payload) {
			Ok(p) => p,
			Err(e) => {
				tracing::error!("[events] could not serialize {event}: {e}");
				return;
			}
		};
		// No receiver connected is not an error: the event is simply dropped,
		// like a Tauri event with no listener.
		let _ = self.tx.send(Event {
			event: event.to_owned(),
			payload,
		});
	}

	pub fn subscribe(&self) -> broadcast::Receiver<Event> {
		self.tx.subscribe()
	}
}

pub async fn handler(
	State(app): State<AppState>,
	ws: WebSocketUpgrade,
) -> Response {
	let hub = app.events.clone();
	ws.on_upgrade(move |socket| pump(socket, hub))
}

async fn pump(mut socket: WebSocket, hub: EventHub) {
	let mut rx = hub.subscribe();
	loop {
		tokio::select! {
			received = rx.recv() => {
				let text = match received {
					Ok(event) => match serde_json::to_string(&event) {
						Ok(text) => text,
						Err(_) => continue,
					},
					Err(broadcast::error::RecvError::Lagged(skipped)) => {
						serde_json::json!({"event": "ws:events-dropped", "payload": skipped})
							.to_string()
					}
					Err(broadcast::error::RecvError::Closed) => break,
				};
				if socket.send(Message::Text(text.into())).await.is_err() {
					break;
				}
			}
			incoming = socket.recv() => match incoming {
				// The browser only listens; anything but a close is ignored.
				Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
				Some(Ok(_)) => {}
			},
		}
	}
}

/// Port of the event forwarding half of `spawn_ws_task`
/// (`src-tauri/src/api/ws.rs`). Session expiry/ban handling arrives with the
/// auth port.
pub fn forward_client_events(client: grindr::GrindrClient, hub: EventHub) {
	{
		let hub = hub.clone();
		let mut rx = client.ws_receiver();
		tokio::spawn(async move {
			loop {
				match rx.recv().await {
					Ok(event) => {
						let safe_type = event.event_type.replace('.', "_");
						hub.emit(&format!("grindr:{safe_type}"), &event.payload);
					}
					Err(broadcast::error::RecvError::Lagged(skipped)) => {
						hub.emit("ws:events-dropped", skipped);
					}
					Err(broadcast::error::RecvError::Closed) => break,
				}
			}
		});
	}
	{
		let mut rx = client.connection_state();
		tokio::spawn(async move {
			loop {
				let name = match *rx.borrow() {
					grindr::WsConnectionState::Connected => "ws:connected",
					grindr::WsConnectionState::Disconnected => "ws:disconnected",
				};
				hub.emit(name, ());
				if rx.changed().await.is_err() {
					break;
				}
			}
		});
	}
}
