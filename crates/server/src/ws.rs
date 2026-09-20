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
                // The hanabii mode is a fixed preset that replaces every
                // other option. Resolve it here, so the room stores — and
                // every client is shown — the concrete rules that will
                // actually be played, and a hand-built `SetRules` can't
                // sneak extra options in next to it.
                let rules = rules.normalized();
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

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::{Color, GameRules};

    type Inbox = mpsc::UnboundedReceiver<ServerMessage>;

    /// A room with `n` seated players, each with an inbox to read what the
    /// server sent them.
    fn room_with_players(n: usize) -> (Arc<Mutex<Room>>, Vec<PlayerId>, Vec<Inbox>) {
        let room = Arc::new(Mutex::new(Room::new()));
        let mut ids = Vec::new();
        let mut inboxes = Vec::new();
        for i in 0..n {
            let (tx, rx) = mpsc::unbounded_channel();
            let id = room.lock().unwrap().add_player(format!("P{i}"), tx).unwrap();
            ids.push(id);
            inboxes.push(rx);
        }
        (room, ids, inboxes)
    }

    fn rules_updates(inbox: &mut Inbox) -> Vec<GameRules> {
        let mut updates = Vec::new();
        while let Ok(msg) = inbox.try_recv() {
            if let ServerMessage::RulesUpdated { rules } = msg {
                updates.push(rules);
            }
        }
        updates
    }

    #[test]
    fn picking_hanabii_locks_the_other_options_and_tells_everyone() {
        let (room, ids, mut inboxes) = room_with_players(2);

        // A client that (buggy, or hand-built) sends every other option
        // alongside the mode.
        let greedy = GameRules {
            multicolor: true,
            black: true,
            extra_colors: 2,
            multicolor_short: true,
            black_short: true,
            extra_colors_short: true,
            six_cards: false,
            hanabii: true,
        };
        handle_client_message(&room, ids[0], ClientMessage::SetRules { rules: greedy });

        let preset = GameRules { hanabii: true, ..Default::default() }.normalized();
        assert_eq!(room.lock().unwrap().rules, preset);
        assert!(preset.hanabii && preset.six_cards && preset.extra_colors == 1);
        assert!(!preset.multicolor && !preset.black);

        // Everyone — the sender included — is shown the locked-in preset,
        // not what was asked for.
        for inbox in &mut inboxes {
            assert_eq!(rules_updates(inbox), vec![preset]);
        }
    }

    #[test]
    fn ordinary_rules_pass_through_untouched() {
        let (room, ids, mut inboxes) = room_with_players(2);
        let rules = GameRules { multicolor: true, extra_colors: 2, black_short: true, ..Default::default() };
        handle_client_message(&room, ids[1], ClientMessage::SetRules { rules });

        assert_eq!(room.lock().unwrap().rules, rules);
        assert_eq!(rules_updates(&mut inboxes[0]), vec![rules]);
    }

    #[test]
    fn unpicking_hanabii_frees_the_options_again() {
        let (room, ids, _inboxes) = room_with_players(2);
        handle_client_message(
            &room,
            ids[0],
            ClientMessage::SetRules { rules: GameRules { hanabii: true, ..Default::default() } },
        );
        // Back to a plain game, then a couple of ordinary options on top.
        let rules = GameRules { black: true, six_cards: true, ..Default::default() };
        handle_client_message(&room, ids[0], ClientMessage::SetRules { rules });
        assert_eq!(room.lock().unwrap().rules, rules);
    }

    #[test]
    fn rules_cannot_change_once_the_game_has_started() {
        let (room, ids, _inboxes) = room_with_players(2);
        handle_client_message(&room, ids[0], ClientMessage::StartGame);
        assert!(room.lock().unwrap().game.is_some());

        handle_client_message(
            &room,
            ids[0],
            ClientMessage::SetRules { rules: GameRules { hanabii: true, ..Default::default() } },
        );
        assert_eq!(room.lock().unwrap().rules, GameRules::default());
    }

    #[test]
    fn a_hanabii_lobby_starts_a_hanabii_game_and_sends_everyone_a_view_of_it() {
        let (room, ids, mut inboxes) = room_with_players(2);
        handle_client_message(
            &room,
            ids[0],
            ClientMessage::SetRules { rules: GameRules { hanabii: true, ..Default::default() } },
        );
        handle_client_message(&room, ids[1], ClientMessage::StartGame);

        {
            let guard = room.lock().unwrap();
            let game = guard.game.as_ref().expect("the game should have started");
            assert!(game.rules.hanabii);
            assert_eq!(game.rules.max_score(), 36);
            assert_eq!(game.fireworks.len(), 6);
            assert!(!game.fireworks.contains_key(&Color::White));
        }

        // The state push is what actually reaches the frontend, which
        // reads it back with serde_json — so check it survives that trip,
        // new fields and all.
        for inbox in &mut inboxes {
            let mut saw_state = false;
            while let Ok(msg) = inbox.try_recv() {
                if let ServerMessage::StateUpdate(view) = msg {
                    let json = serde_json::to_string(&ServerMessage::StateUpdate(view.clone())).unwrap();
                    let Ok(ServerMessage::StateUpdate(back)) = serde_json::from_str::<ServerMessage>(&json)
                    else {
                        panic!("state update didn't round-trip: {json}");
                    };
                    assert_eq!(back.rules, view.rules);
                    assert!(back.rules.hanabii);
                    assert_eq!(back.fireworks, view.fireworks);
                    saw_state = true;
                }
            }
            assert!(saw_state, "every player should get a state update when the game starts");
        }
    }

    #[test]
    fn rules_payloads_from_before_hanabii_existed_still_parse() {
        // `#[serde(default)]` on every field: a client that has never heard
        // of the mode just doesn't send it, and gets a normal game.
        let old: GameRules = serde_json::from_str(r#"{"multicolor":true,"extra_colors":1}"#).unwrap();
        assert!(old.multicolor && !old.hanabii);

        let bare: GameRules = serde_json::from_str("{}").unwrap();
        assert_eq!(bare, GameRules::default());

        let json = serde_json::to_string(&GameRules { hanabii: true, ..Default::default() }).unwrap();
        assert!(serde_json::from_str::<GameRules>(&json).unwrap().hanabii);
    }
}
