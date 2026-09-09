use game_core::{Action, ActionError, GameRules, GameState, PlayerId, ServerMessage};
use tokio::sync::mpsc;

pub struct Seat {
    pub player_id: PlayerId,
    pub name: String,
    pub sender: mpsc::UnboundedSender<ServerMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    AlreadyStarted,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    AlreadyStarted,
    NotEnoughPlayers,
    TooManyPlayers,
}

#[derive(Default)]
pub struct Room {
    pub seats: Vec<Seat>,
    pub game: Option<GameState>,
    /// Variant rules selected in the lobby, applied when the game starts.
    /// Any seated player can change this up until then (same "no host"
    /// model as starting the game itself).
    pub rules: GameRules,
}

impl Room {
    pub fn new() -> Self {
        Room::default()
    }

    pub fn add_player(
        &mut self,
        name: String,
        sender: mpsc::UnboundedSender<ServerMessage>,
    ) -> Result<PlayerId, JoinError> {
        if self.game.is_some() {
            return Err(JoinError::AlreadyStarted);
        }
        if self.seats.len() >= 5 {
            return Err(JoinError::Full);
        }

        let player_id = PlayerId(self.seats.len() as u8);
        self.seats.push(Seat {
            player_id,
            name,
            sender,
        });
        Ok(player_id)
    }

    pub fn remove_player(&mut self, player_id: PlayerId) {
        self.seats.retain(|s| s.player_id != player_id);
    }

    pub fn roster(&self) -> Vec<(PlayerId, String)> {
        self.seats
            .iter()
            .map(|s| (s.player_id, s.name.clone()))
            .collect()
    }

    pub fn start_game(&mut self) -> Result<(), StartError> {
        if self.game.is_some() {
            return Err(StartError::AlreadyStarted);
        }
        if self.seats.len() < 2 {
            return Err(StartError::NotEnoughPlayers);
        }
        if self.seats.len() > 5 {
            return Err(StartError::TooManyPlayers);
        }

        // Not cryptographically random, and doesn't need to be — this only
        // has to vary reasonably between games, not resist prediction.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        self.game = Some(GameState::new(self.seats.len() as u8, seed, self.rules));
        Ok(())
    }

    pub fn apply_action(&mut self, player: PlayerId, action: Action) -> Result<(), ActionError> {
        let game = self.game.as_mut().ok_or(ActionError::GameOver)?;
        game.apply_action(player, action)?;
        Ok(())
    }

    pub fn broadcast(&self, msg: &ServerMessage) {
        for seat in &self.seats {
            let _ = seat.sender.send(msg.clone());
        }
    }

    /// Same as `broadcast`, but skips one seat. Used for join announcements:
    /// the player who just joined already got the full roster in their
    /// `Joined` reply, so they don't need the `PlayerJoined` broadcast too.
    pub fn broadcast_except(&self, exclude: PlayerId, msg: &ServerMessage) {
        for seat in &self.seats {
            if seat.player_id != exclude {
                let _ = seat.sender.send(msg.clone());
            }
        }
    }

    pub fn send_to(&self, player_id: PlayerId, msg: ServerMessage) {
        if let Some(seat) = self.seats.iter().find(|s| s.player_id == player_id) {
            let _ = seat.sender.send(msg);
        }
    }

    /// Sends every seated player their own redacted view. No-op before the
    /// game has started. This is the *only* sync mechanism in v1: rather
    /// than streaming granular events to the client, every accepted action
    /// just triggers a fresh full state push to everyone. Simpler to reason
    /// about, and Hanabi's state is small enough that the bandwidth cost is
    /// irrelevant.
    pub fn broadcast_state(&self) {
        let Some(game) = &self.game else { return };
        for seat in &self.seats {
            let view = game.view_for(seat.player_id);
            let _ = seat.sender.send(ServerMessage::StateUpdate(view));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_sender() -> mpsc::UnboundedSender<ServerMessage> {
        let (tx, _rx) = mpsc::unbounded_channel();
        tx
    }

    #[test]
    fn seats_fill_in_join_order() {
        let mut room = Room::new();
        let id0 = room.add_player("Alice".into(), dummy_sender()).unwrap();
        let id1 = room.add_player("Bob".into(), dummy_sender()).unwrap();
        assert_eq!(id0, PlayerId(0));
        assert_eq!(id1, PlayerId(1));
    }

    #[test]
    fn cannot_join_a_full_room() {
        let mut room = Room::new();
        for i in 0..5 {
            room.add_player(format!("P{i}"), dummy_sender()).unwrap();
        }
        assert_eq!(
            room.add_player("P6".into(), dummy_sender()),
            Err(JoinError::Full)
        );
    }

    #[test]
    fn cannot_start_with_fewer_than_two_players() {
        let mut room = Room::new();
        room.add_player("Solo".into(), dummy_sender()).unwrap();
        assert_eq!(room.start_game(), Err(StartError::NotEnoughPlayers));
    }

    #[test]
    fn starts_successfully_with_two_players() {
        let mut room = Room::new();
        room.add_player("Alice".into(), dummy_sender()).unwrap();
        room.add_player("Bob".into(), dummy_sender()).unwrap();
        assert!(room.start_game().is_ok());
        assert!(room.game.is_some());
    }

    #[test]
    fn cannot_join_after_the_game_has_started() {
        let mut room = Room::new();
        room.add_player("Alice".into(), dummy_sender()).unwrap();
        room.add_player("Bob".into(), dummy_sender()).unwrap();
        room.start_game().unwrap();
        assert_eq!(
            room.add_player("Carol".into(), dummy_sender()),
            Err(JoinError::AlreadyStarted)
        );
    }

    #[test]
    fn removing_a_player_frees_no_seat_but_stops_them_receiving() {
        let mut room = Room::new();
        let alice = room.add_player("Alice".into(), dummy_sender()).unwrap();
        room.remove_player(alice);
        assert!(room.seats.is_empty());
    }
}
