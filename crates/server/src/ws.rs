use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use game_core::{ClientMessage, PlayerId, ServerMessage};

use crate::room::{JoinError, Room, StartError};
use crate::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    println!("[ws] new connection");

    // The first message on any connection must be a Join; ignore anything
    // else (pings etc.) until we get one, and bail if the connection closes
    // before that happens.
    let (room_code, name) = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Join { room_code, name }) => break (room_code, name),
                    Ok(other) => {
                        println!("[ws] first message wasn't a Join, ignoring: {other:?}");
                    }
                    Err(e) => {
                        println!("[ws] first message didn't parse as ClientMessage: {e} (raw: {text})");
                    }
                }
            }
            Some(Ok(_)) => continue,
            _ => {
                println!("[ws] connection closed before sending a Join");
                return;
            }
        }
    };
    println!("[ws] join request: room={room_code:?} name={name:?}");

    let room = state.get_or_create_room(&room_code);
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    // All lock-holding work happens synchronously in this block, so the
    // guard is always dropped before any `.await` — holding a std Mutex
    // guard across an await point is a bug (and won't compile once the
    // surrounding future needs to be Send).
    let join_result = {
        let mut room_guard = room.lock().unwrap();
        let result = room_guard.add_player(name.clone(), tx.clone());
        if let Ok(id) = result {
            let roster = room_guard.roster();
            let _ = tx.send(ServerMessage::Joined {
                you: id,
                players: roster,
                rules: room_guard.rules,
            });
            room_guard.broadcast_except(
                id,
                &ServerMessage::PlayerJoined {
                    player_id: id,
                    name: name.clone(),
                },
            );
        }
        result
    };

    let player_id = match join_result {
        Ok(id) => id,
        Err(reason) => {
            let message = match reason {
                JoinError::AlreadyStarted => "That game has already started.",
                JoinError::Full => "That room is full.",
            }
            .to_string();
            let payload = serde_json::to_string(&ServerMessage::Error { message })
                .unwrap_or_default();
            let _ = socket.send(Message::Text(payload)).await;
            let _ = socket.close().await;
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(json) = serde_json::to_string(&msg) else {
                continue;
            };
            if ws_sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    let room_for_recv = room.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
            let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
                continue;
            };
            handle_client_message(&room_for_recv, player_id, client_msg);
        }
    });

    // Whichever of send/receive ends first (client disconnected, or the
    // outbound channel closed) means this connection is done; cancel the
    // other half rather than leaking it.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    let mut room_guard = room.lock().unwrap();
    room_guard.remove_player(player_id);
    room_guard.broadcast(&ServerMessage::PlayerLeft { player_id });
}

/// Handles one already-parsed message from an already-joined player. Plain
/// sync function: every branch does its locked work and returns without
/// ever awaiting, so there's nothing async here to reason about.
fn handle_client_message(room: &Arc<Mutex<Room>>, player_id: PlayerId, msg: ClientMessage) {
    let mut room_guard = room.lock().unwrap();

    match msg {
        ClientMessage::Join { .. } => {
            // Already joined on this connection; a second Join is ignored.
        }
        ClientMessage::SetRules { rules } => {
            // Lobby-only; a stray SetRules after the game has started is
            // just ignored rather than erroring, same tolerant handling as
            // a repeated Join above.
            if room_guard.game.is_none() {
                room_guard.rules = rules;
                room_guard.broadcast(&ServerMessage::RulesUpdated { rules });
            }
        }
        ClientMessage::StartGame => match room_guard.start_game() {
            Ok(()) => {
                room_guard.broadcast(&ServerMessage::GameStarted);
                room_guard.broadcast_state();
            }
            Err(reason) => {
                let message = match reason {
                    StartError::AlreadyStarted => "The game has already started.",
                    StartError::NotEnoughPlayers => "Need at least 2 players to start.",
                    StartError::TooManyPlayers => "Hanabi only supports up to 5 players.",
                }
                .to_string();
                room_guard.send_to(player_id, ServerMessage::Error { message });
            }
        },
        ClientMessage::Action(action) => match room_guard.apply_action(player_id, action) {
            Ok(()) => room_guard.broadcast_state(),
            Err(err) => {
                room_guard.send_to(
                    player_id,
                    ServerMessage::ActionRejected {
                        reason: format!("{err:?}"),
                    },
                );
            }
        },
    }
}
