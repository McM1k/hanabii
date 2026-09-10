use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::card::{Card, CardId, Color, Number};
use crate::knowledge::CardKnowledge;
use crate::player::PlayerId;
use crate::rules::GameRules;
use crate::state::{GameState, GameStatus, LastMove};

/// A card as seen by one particular viewer: the face is hidden for a
/// player's own un-clued cards, visible for everyone else's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleCard {
    pub id: CardId,
    pub card: Option<Card>,
    pub knowledge: CardKnowledge,
}

/// The redacted, per-player view sent down the wire. This is the *only*
/// representation of game state a client ever receives — the server never
/// sends the full `GameState`, so there's no way to peek at your own hand
/// even by inspecting network traffic in devtools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerView {
    pub you: PlayerId,
    pub current_turn: PlayerId,
    pub hands: HashMap<PlayerId, Vec<VisibleCard>>,
    pub fireworks: HashMap<Color, Number>,
    pub clue_tokens: u8,
    pub fuse_tokens: u8,
    pub draw_pile_count: usize,
    pub discard_pile: Vec<Card>,
    pub status: GameStatus,
    pub score: u8,
    pub last_moves: HashMap<PlayerId, LastMove>,
    /// The variant rules this game was started with — lets the client know,
    /// for instance, which colors are actually in play, without having to
    /// infer it from `fireworks`' (arbitrarily-ordered) keys.
    pub rules: GameRules,
}

impl GameState {
    pub fn view_for(&self, viewer: PlayerId) -> PlayerView {
        let hands: HashMap<PlayerId, Vec<VisibleCard>> = self
            .hands
            .iter()
            .map(|(&pid, hand)| {
                let visible: Vec<VisibleCard> = hand
                    .iter()
                    .map(|hc| VisibleCard {
                        id: hc.id,
                        card: if pid == viewer { None } else { Some(hc.card) },
                        knowledge: hc.knowledge.clone(),
                    })
                    .collect();
                (pid, visible)
            })
            .collect();

        PlayerView {
            you: viewer,
            current_turn: self.current_player(),
            hands,
            fireworks: self.fireworks.clone(),
            clue_tokens: self.clue_tokens,
            fuse_tokens: self.fuse_tokens,
            draw_pile_count: self.draw_pile.len(),
            discard_pile: self.discard_pile.clone(),
            status: self.status,
            score: self.score(),
            last_moves: self.last_moves.clone(),
            rules: self.rules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_hand_is_redacted_others_are_not() {
        let g = GameState::new(2, 5, GameRules::default());
        let view = g.view_for(PlayerId(0));

        assert!(view.hands[&PlayerId(0)].iter().all(|c| c.card.is_none()));
        assert!(view.hands[&PlayerId(1)].iter().all(|c| c.card.is_some()));
    }

    #[test]
    fn clued_information_is_still_visible_without_the_card_itself() {
        let mut g = GameState::new(2, 5, GameRules::default());

        // Player 0 always acts first (current_turn starts at 0). Give a
        // throwaway clue to pass the turn to player 1 before testing the
        // thing we actually care about: player 1 clueing player 0.
        let p1_color = g.hands[&PlayerId(1)][0].card.color;
        g.apply_action(
            PlayerId(0),
            crate::state::Action::Clue {
                target: PlayerId(1),
                clue: crate::card::Clue::Color(p1_color),
            },
        )
        .unwrap();

        let color = g.hands[&PlayerId(0)][0].card.color;
        g.apply_action(
            PlayerId(1),
            crate::state::Action::Clue {
                target: PlayerId(0),
                clue: crate::card::Clue::Color(color),
            },
        )
        .unwrap();

        let view = g.view_for(PlayerId(0));
        assert_eq!(view.hands[&PlayerId(0)][0].card, None);
        assert_eq!(view.hands[&PlayerId(0)][0].knowledge.known_color, Some(color));
    }

    #[test]
    fn view_carries_the_game_rules() {
        let g = GameState::new(2, 5, GameRules { multicolor: true, black: false });
        let view = g.view_for(PlayerId(0));
        assert_eq!(view.rules, GameRules { multicolor: true, black: false });
        assert_eq!(view.fireworks.len(), 6);
    }
}
