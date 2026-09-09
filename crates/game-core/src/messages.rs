use serde::{Deserialize, Serialize};

use crate::player::PlayerId;
use crate::protocol::PlayerView;
use crate::rules::GameRules;
use crate::state::Action;

/// Sent from a client to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The first message any connection must send. Creates the room if it
    /// doesn't exist yet, or joins it if it does (and isn't full/started).
    Join { room_code: String, name: String },
    /// Lobby-only: replaces the room's selected variant rules wholesale.
    /// Any seated player can send this before the game starts — same "no
    /// host" model as `StartGame` — and the server echoes the update back
    /// to everyone so every lobby's toggles stay in sync. Ignored if the
    /// game has already started.
    SetRules { rules: GameRules },
    /// Any seated player can trigger this once there are at least 2 players
    /// in the room — there's no separate "host" concept in v1.
    StartGame,
    /// A normal game move once the game is running.
    Action(Action),
}

/// Sent from the server to one or more clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Reply to a successful Join, sent only to the joining client. Includes
    /// whatever rules are currently selected, so a player joining mid-lobby
    /// sees the existing toggles immediately rather than a stray default.
    Joined {
        you: PlayerId,
        players: Vec<(PlayerId, String)>,
        rules: GameRules,
    },
    /// Broadcast to everyone already in the room when someone new joins.
    PlayerJoined { player_id: PlayerId, name: String },
    /// Broadcast when a connection drops.
    PlayerLeft { player_id: PlayerId },
    /// Broadcast whenever the room's selected rules change.
    RulesUpdated { rules: GameRules },
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
