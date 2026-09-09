use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::*;

use game_core::{ClientMessage, GameRules, PlayerId, PlayerView, ServerMessage};

const SERVER_URL: &str = "ws://localhost:3000/ws";

/// App-wide reactive state, provided once at the root via `provide_context`
/// and read from anywhere below via `use_context`. Every field is a Leptos
/// signal, which are `Copy` — that's why this whole struct can derive `Copy`
/// too, and be freely captured into as many closures as needed without
/// fighting the borrow checker.
#[derive(Clone, Copy)]
pub struct AppContext {
    pub my_id: RwSignal<Option<PlayerId>>,
    pub roster: RwSignal<Vec<(PlayerId, String)>>,
    pub view: RwSignal<Option<PlayerView>>,
    pub status: RwSignal<String>,
    pub game_started: RwSignal<bool>,
    /// The room's currently-selected variant rules. Live in the lobby (any
    /// seat can toggle these, synced via `RulesUpdated`); frozen for the
    /// rest of the game once `GameStarted` arrives.
    pub rules: RwSignal<GameRules>,
    outbound: RwSignal<Option<mpsc::UnboundedSender<ClientMessage>>>,
}

impl AppContext {
    pub fn new() -> Self {
        AppContext {
            my_id: create_rw_signal(None),
            roster: create_rw_signal(Vec::new()),
            view: create_rw_signal(None),
            status: create_rw_signal(String::new()),
            game_started: create_rw_signal(false),
            rules: create_rw_signal(GameRules::default()),
            outbound: create_rw_signal(None),
        }
    }

    /// Sends a message to the server if we're connected. There's no path
    /// through this UI where an action is clickable before a connection
    /// exists, so a silently-dropped send when disconnected is fine.
    pub fn send(&self, msg: ClientMessage) {
        if let Some(tx) = self.outbound.get_untracked() {
            let _ = tx.unbounded_send(msg);
        }
    }
}

/// Opens the WebSocket, spawns the two background tasks that pump messages
/// in and out of it, and sends the initial Join. Mirrors the server's own
/// split-socket pattern, but with `futures::channel::mpsc` instead of
/// `tokio::sync::mpsc` — there's no Tokio runtime in a browser.
pub fn connect(ctx: AppContext, room_code: String, name: String) {
    let ws = match WebSocket::open(SERVER_URL) {
        Ok(ws) => ws,
        Err(_) => {
            ctx.status
                .set(format!("Couldn't reach the server at {SERVER_URL}."));
            return;
        }
    };

    let (mut write, mut read) = ws.split();
    let (tx, mut rx) = mpsc::unbounded::<ClientMessage>();

    // Outbound pump: local UI events push onto `rx` synchronously via
    // `unbounded_send`; this task is the only thing that actually touches
    // the socket's write half.
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(msg) = rx.next().await {
            let Ok(json) = serde_json::to_string(&msg) else {
                continue;
            };
            if write.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Inbound pump: every server message updates the shared signals, which
    // is all it takes for Leptos to re-render whatever reads them.
    wasm_bindgen_futures::spawn_local(async move {
        let mut disconnect_reason = "Disconnected from the server.".to_string();
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                        apply_server_message(ctx, server_msg);
                    } else {
                        leptos::logging::warn!("couldn't parse server message: {text}");
                    }
                }
                Ok(Message::Bytes(_)) => {}
                Err(e) => {
                    disconnect_reason = format!("Disconnected from the server: {e}");
                    break;
                }
            }
        }
        ctx.status.set(disconnect_reason);
    });

    ctx.outbound.set(Some(tx.clone()));
    let _ = tx.unbounded_send(ClientMessage::Join { room_code, name });
}

fn apply_server_message(ctx: AppContext, msg: ServerMessage) {
    match msg {
        ServerMessage::Joined { you, players, rules } => {
            ctx.my_id.set(Some(you));
            ctx.roster.set(players);
            ctx.rules.set(rules);
        }
        ServerMessage::PlayerJoined { player_id, name } => {
            ctx.roster.update(|r| r.push((player_id, name)));
        }
        ServerMessage::PlayerLeft { player_id } => {
            ctx.roster.update(|r| r.retain(|(id, _)| *id != player_id));
        }
        ServerMessage::RulesUpdated { rules } => {
            ctx.rules.set(rules);
        }
        ServerMessage::GameStarted => {
            ctx.game_started.set(true);
        }
        ServerMessage::StateUpdate(view) => {
            ctx.view.set(Some(view));
        }
        ServerMessage::ActionRejected { reason } => {
            ctx.status.set(format!("That move wasn't allowed: {reason}"));
        }
        ServerMessage::Error { message } => {
            ctx.status.set(message);
        }
    }
}
