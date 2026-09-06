mod room;
mod ws;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;

use room::Room;

pub struct AppState {
    rooms: Mutex<HashMap<String, Arc<Mutex<Room>>>>,
}

impl AppState {
    fn new() -> Self {
        AppState {
            rooms: Mutex::new(HashMap::new()),
        }
    }

    /// Gets the room for this code, creating an empty one if it doesn't
    /// exist yet. Room codes are just whatever string the first player to
    /// use them sends — there's no separate "create room" step.
    pub fn get_or_create_room(&self, room_code: &str) -> Arc<Mutex<Room>> {
        let mut rooms = self.rooms.lock().unwrap();
        rooms
            .entry(room_code.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Room::new())))
            .clone()
    }
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let app = Router::new()
        .route("/ws", get(ws::ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("Hanabi server listening on ws://0.0.0.0:3000/ws");

    axum::serve(listener, app).await.expect("server error");
}
