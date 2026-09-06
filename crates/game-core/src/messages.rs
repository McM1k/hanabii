use serde::{Deserialize, Serialize};

use crate::player::PlayerId;
use crate::protocol::PlayerView;
use crate::state::Action;

/// Sent from a client to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The first message any connection must send. Creates the room if it
    /// doesn't exist yet, or joins it if it does (and isn't full/started).
    Join { room_code: String, name: String },
    /// Any seated player can trigger this once there are at least 2 players
    /// in the room — there's no separate "host" concept in v1.
    StartGame,
    /// A normal game move once the game is running.
    Action(Action),
}

/// Sent from the server to one or more clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Reply to a successful Join, sent only to the joining client.
    Joined {
        you: PlayerId,
        players: Vec<(PlayerId, String)>,
    },
    /// Broadcast to everyone already in the room when someone new joins.
    PlayerJoined { player_id: PlayerId, name: String },
    /// Broadcast when a connection drops.
    PlayerLeft { player_id: PlayerId },
    /// Broadcast once the game transitions from lobby to in-progress.
    GameStarted,
    /// The redacted state update, personalized per recipient. The whole
    /// point of this type existing is that two players never receive the
    /// same payload for the same event.
    StateUpdate(PlayerView),
    /// Sent only to the client whose action was illegal, with a
    /// human-readable reason (built from `Debug` on `ActionError` for now —
    /// fine for a v1, worth prettifying once the frontend needs to show it).
    ActionRejected { reason: String },
    /// Anything else that couldn't be handled — bad room code, tried to
    /// start with 1 player, malformed message, etc.
    Error { message: String },
}
