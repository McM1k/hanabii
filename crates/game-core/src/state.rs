use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::card::{Card, CardId, Clue, Color, Number};
use crate::deck::shuffled_deck;
use crate::knowledge::CardKnowledge;
use crate::player::PlayerId;
use crate::rules::GameRules;

pub const MAX_CLUE_TOKENS: u8 = 8;
pub const MAX_FUSE_TOKENS: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandCard {
    pub id: CardId,
    pub card: Card,
    pub knowledge: CardKnowledge,
}

/// A summary of the most recent thing a player did, kept per-player so the
/// UI can show everyone's last move at once. This matters for conventions
/// like finesses, where correctly reading a clue depends on knowing exactly
/// what happened on recent turns, not just the current board state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LastMove {
    Clue {
        target: PlayerId,
        clue: Clue,
        touched_count: usize,
    },
    Play {
        card: Card,
        success: bool,
    },
    Discard {
        card: Card,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameStatus {
    InProgress,
    Finished(EndReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    FusesExhausted,
    DeckExhausted,
    PerfectScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Clue { target: PlayerId, clue: Clue },
    Play { card_id: CardId },
    Discard { card_id: CardId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionError {
    NotYourTurn,
    GameOver,
    NoClueTokens,
    CannotClueSelf,
    UnknownPlayer,
    /// The multicolor suit can never be clued directly, even when the
    /// multicolor rule is on — only the five base colors can.
    CannotClueMulticolor,
    ClueMatchesNothing,
    CardNotInHand,
    CannotDiscardAtMaxClues,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    ClueGiven {
        from: PlayerId,
        target: PlayerId,
        clue: Clue,
        touched: Vec<CardId>,
    },
    CardPlayed {
        player: PlayerId,
        card_id: CardId,
        card: Card,
        success: bool,
    },
    CardDiscarded {
        player: PlayerId,
        card_id: CardId,
        card: Card,
    },
    CardDrawn {
        player: PlayerId,
        card_id: CardId,
    },
    GameOver {
        status: GameStatus,
        score: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub players: Vec<PlayerId>,
    pub hands: HashMap<PlayerId, Vec<HandCard>>,
    pub draw_pile: Vec<Card>,
    pub discard_pile: Vec<Card>,
    pub fireworks: HashMap<Color, Number>,
    pub clue_tokens: u8,
    pub fuse_tokens: u8,
    pub current_turn: usize,
    pub status: GameStatus,
    pub final_round_starting_player: Option<usize>,
    pub last_moves: HashMap<PlayerId, LastMove>,
    pub rules: GameRules,
    next_card_id: u32,
}

impl GameState {
    pub fn new(player_count: u8, seed: u64, rules: GameRules) -> Self {
        assert!(
            (2..=5).contains(&player_count),
            "Hanabi supports 2-5 players"
        );
        let players: Vec<PlayerId> = (0..player_count).map(PlayerId).collect();
        let hand_size = if player_count <= 3 { 5 } else { 4 };

        let mut draw_pile = shuffled_deck(seed, &rules);
        let mut hands = HashMap::new();
        let mut next_card_id = 0u32;

        for &p in &players {
            let mut hand = Vec::new();
            for _ in 0..hand_size {
                let card = draw_pile
                    .pop()
                    .expect("standard deck always has enough cards for the initial deal");
                hand.push(HandCard {
                    id: CardId(next_card_id),
                    card,
                    knowledge: CardKnowledge::default(),
                });
                next_card_id += 1;
            }
            hands.insert(p, hand);
        }

        let fireworks: HashMap<Color, Number> =
            rules.active_colors().into_iter().map(|c| (c, 0)).collect();

        GameState {
            players,
            hands,
            draw_pile,
            discard_pile: Vec::new(),
            fireworks,
            clue_tokens: MAX_CLUE_TOKENS,
            fuse_tokens: MAX_FUSE_TOKENS,
            current_turn: 0,
            status: GameStatus::InProgress,
            final_round_starting_player: None,
            last_moves: HashMap::new(),
            rules,
            next_card_id,
        }
    }

    pub fn current_player(&self) -> PlayerId {
        self.players[self.current_turn]
    }

    fn player_index(&self, player: PlayerId) -> Option<usize> {
        self.players.iter().position(|&p| p == player)
    }

    pub fn apply_action(
        &mut self,
        player: PlayerId,
        action: Action,
    ) -> Result<Vec<Event>, ActionError> {
        if self.status != GameStatus::InProgress {
            return Err(ActionError::GameOver);
        }
        if player != self.current_player() {
            return Err(ActionError::NotYourTurn);
        }

        let mut events = match action {
            Action::Clue { target, clue } => self.apply_clue(player, target, clue)?,
            Action::Play { card_id } => self.apply_play(player, card_id)?,
            Action::Discard { card_id } => self.apply_discard(player, card_id)?,
        };

        self.advance_turn();

        if let Some(over) = self.check_game_over() {
            events.push(over);
        }

        Ok(events)
    }

    fn apply_clue(
        &mut self,
        from: PlayerId,
        target: PlayerId,
        clue: Clue,
    ) -> Result<Vec<Event>, ActionError> {
        if target == from {
            return Err(ActionError::CannotClueSelf);
        }
        if self.player_index(target).is_none() {
            return Err(ActionError::UnknownPlayer);
        }
        if matches!(clue, Clue::Color(Color::Multicolor)) {
            // Standard multicolor-suit rule: it's wild when *receiving* a
            // clue (see the is_match arm below), but can never be the color
            // named in a clue.
            return Err(ActionError::CannotClueMulticolor);
        }
        if self.clue_tokens == 0 {
            return Err(ActionError::NoClueTokens);
        }

        let hand = self
            .hands
            .get_mut(&target)
            .expect("every seated player has a hand");

        let mut touched = Vec::new();
        let mut any_match = false;

        for hc in hand.iter_mut() {
            let is_match = match clue {
                // A multicolor card counts as every color for clue-matching
                // purposes, so a "Red" clue touches actual red cards *and*
                // any multicolor cards in the hand.
                Clue::Color(c) => hc.card.color == c || hc.card.color == Color::Multicolor,
                Clue::Number(n) => hc.card.number == n,
            };
            if is_match {
                any_match = true;
                touched.push(hc.id);
                hc.knowledge.apply_positive(clue);
            } else {
                hc.knowledge.apply_negative(clue);
            }
        }

        if !any_match {
            return Err(ActionError::ClueMatchesNothing);
        }

        self.clue_tokens -= 1;

        self.last_moves.insert(
            from,
            LastMove::Clue {
                target,
                clue,
                touched_count: touched.len(),
            },
        );

        Ok(vec![Event::ClueGiven {
            from,
            target,
            clue,
            touched,
        }])
    }

    fn apply_play(&mut self, player: PlayerId, card_id: CardId) -> Result<Vec<Event>, ActionError> {
        let (played, idx) = self.take_card(player, card_id)?;
        self.hands.get_mut(&player).unwrap().remove(idx);

        let top = *self.fireworks.get(&played.card.color).unwrap();
        let success = played.card.number == top + 1;

        if success {
            self.fireworks.insert(played.card.color, played.card.number);
            if played.card.number == 5 && self.clue_tokens < MAX_CLUE_TOKENS {
                self.clue_tokens += 1;
            }
        } else {
            self.discard_pile.push(played.card);
            self.fuse_tokens = self.fuse_tokens.saturating_sub(1);
        }

        self.last_moves.insert(
            player,
            LastMove::Play {
                card: played.card,
                success,
            },
        );

        let mut events = vec![Event::CardPlayed {
            player,
            card_id,
            card: played.card,
            success,
        }];
        events.extend(self.draw_replacement(player));
        Ok(events)
    }

    fn apply_discard(
        &mut self,
        player: PlayerId,
        card_id: CardId,
    ) -> Result<Vec<Event>, ActionError> {
        if self.clue_tokens >= MAX_CLUE_TOKENS {
            return Err(ActionError::CannotDiscardAtMaxClues);
        }

        let (discarded, idx) = self.take_card(player, card_id)?;
        self.hands.get_mut(&player).unwrap().remove(idx);

        self.discard_pile.push(discarded.card);
        self.clue_tokens += 1;

        self.last_moves.insert(
            player,
            LastMove::Discard {
                card: discarded.card,
            },
        );

        let mut events = vec![Event::CardDiscarded {
            player,
            card_id,
            card: discarded.card,
        }];
        events.extend(self.draw_replacement(player));
        Ok(events)
    }

    fn take_card(
        &self,
        player: PlayerId,
        card_id: CardId,
    ) -> Result<(HandCard, usize), ActionError> {
        let hand = self.hands.get(&player).ok_or(ActionError::UnknownPlayer)?;
        let idx = hand
            .iter()
            .position(|hc| hc.id == card_id)
            .ok_or(ActionError::CardNotInHand)?;
        Ok((hand[idx].clone(), idx))
    }

    fn draw_replacement(&mut self, player: PlayerId) -> Vec<Event> {
        if let Some(card) = self.draw_pile.pop() {
            let id = CardId(self.next_card_id);
            self.next_card_id += 1;
            self.hands.get_mut(&player).unwrap().push(HandCard {
                id,
                card,
                knowledge: CardKnowledge::default(),
            });

            if self.draw_pile.is_empty() && self.final_round_starting_player.is_none() {
                self.final_round_starting_player = Some(self.current_turn);
            }

            vec![Event::CardDrawn { player, card_id: id }]
        } else {
            if self.final_round_starting_player.is_none() {
                self.final_round_starting_player = Some(self.current_turn);
            }
            vec![]
        }
    }

    fn advance_turn(&mut self) {
        self.current_turn = (self.current_turn + 1) % self.players.len();
    }

    /// Checks and, if applicable, applies the game-over transition. Order
    /// matters: running out of fuses or completing every firework ends the
    /// game immediately, even mid final-round; otherwise the final round
    /// (one extra turn per player after the deck empties) has to actually
    /// wrap back around to whoever drew the last card.
    fn check_game_over(&mut self) -> Option<Event> {
        if self.status != GameStatus::InProgress {
            return None;
        }

        if self.fuse_tokens == 0 {
            self.status = GameStatus::Finished(EndReason::FusesExhausted);
        } else if self.fireworks.values().all(|&n| n == 5) {
            self.status = GameStatus::Finished(EndReason::PerfectScore);
        } else if self.final_round_starting_player == Some(self.current_turn) {
            self.status = GameStatus::Finished(EndReason::DeckExhausted);
        }

        match self.status {
            GameStatus::InProgress => None,
            GameStatus::Finished(_) => Some(Event::GameOver {
                status: self.status,
                score: self.score(),
            }),
        }
    }

    pub fn score(&self) -> u8 {
        self.fireworks.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_player_game() -> GameState {
        GameState::new(2, 42, GameRules::default())
    }

    #[test]
    fn deals_correct_hand_sizes() {
        let g2 = GameState::new(2, 1, GameRules::default());
        assert_eq!(g2.hands[&PlayerId(0)].len(), 5);
        assert_eq!(g2.hands[&PlayerId(1)].len(), 5);

        let g4 = GameState::new(4, 1, GameRules::default());
        for p in &g4.players {
            assert_eq!(g4.hands[p].len(), 4);
        }
    }

    #[test]
    fn dealt_plus_draw_pile_equals_fifty() {
        let g = GameState::new(3, 7, GameRules::default());
        let dealt: usize = g.hands.values().map(|h| h.len()).sum();
        assert_eq!(dealt + g.draw_pile.len(), 50);
    }

    #[test]
    fn multicolor_rule_deals_from_a_sixty_card_deck() {
        let g = GameState::new(3, 7, GameRules { multicolor: true });
        let dealt: usize = g.hands.values().map(|h| h.len()).sum();
        assert_eq!(dealt + g.draw_pile.len(), 60);
        assert_eq!(g.fireworks.len(), 6);
        assert_eq!(g.fireworks.get(&Color::Multicolor), Some(&0));
    }

    #[test]
    fn clue_must_touch_at_least_one_card() {
        let mut g = two_player_game();
        let present: std::collections::HashSet<Color> = g.hands[&PlayerId(1)]
            .iter()
            .map(|hc| hc.card.color)
            .collect();
        let missing = Color::ALL.into_iter().find(|c| !present.contains(c));

        if let Some(color) = missing {
            let result = g.apply_action(
                PlayerId(0),
                Action::Clue {
                    target: PlayerId(1),
                    clue: Clue::Color(color),
                },
            );
            assert_eq!(result.unwrap_err(), ActionError::ClueMatchesNothing);
        }
    }

    #[test]
    fn clueing_spends_a_token_and_marks_cards() {
        let mut g = two_player_game();
        let target_card = g.hands[&PlayerId(1)][0].clone();

        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(target_card.card.color),
            },
        )
        .unwrap();

        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS - 1);
        let updated = &g.hands[&PlayerId(1)][0];
        assert_eq!(updated.knowledge.known_color, Some(target_card.card.color));
    }

    #[test]
    fn multicolor_card_is_touched_by_any_color_clue() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: true });
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Multicolor,
            number: 2,
        };
        let multi_card_id = g.hands[&PlayerId(1)][0].id;

        let events = g
            .apply_action(
                PlayerId(0),
                Action::Clue {
                    target: PlayerId(1),
                    clue: Clue::Color(Color::Green),
                },
            )
            .unwrap();

        match &events[0] {
            Event::ClueGiven { touched, .. } => assert!(touched.contains(&multi_card_id)),
            other => panic!("expected a ClueGiven event, got {other:?}"),
        }
        let knowledge = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == multi_card_id)
            .unwrap()
            .knowledge;
        // The clued color is recorded even though the card is actually
        // multicolor — that ambiguity is the whole point of the variant.
        assert_eq!(knowledge.known_color, Some(Color::Green));
    }

    #[test]
    fn cannot_clue_multicolor_directly() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: true });
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::Multicolor),
            },
        );
        assert_eq!(result.unwrap_err(), ActionError::CannotClueMulticolor);
    }

    #[test]
    fn two_different_color_clues_on_the_same_card_reveal_it_as_multicolor() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: true });
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Multicolor,
            number: 3,
        };
        // Pinned so the "pass the turn back" clue below is guaranteed to
        // touch something, regardless of what the seed happened to deal.
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::Red,
            number: 1,
        };
        let card_id = g.hands[&PlayerId(1)][0].id;

        // Turn 1: clue the multicolor card about Green.
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::Green),
            },
        )
        .unwrap();
        let knowledge_after_one_clue = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == card_id)
            .unwrap()
            .knowledge;
        assert!(!knowledge_after_one_clue.inferred_multicolor());

        // Turn 2: just pass the turn back.
        g.apply_action(
            PlayerId(1),
            Action::Clue {
                target: PlayerId(0),
                clue: Clue::Number(1),
            },
        )
        .unwrap();

        // Turn 3: clue the same card again, this time about a *different*
        // color — only the multicolor suit could match both.
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::White),
            },
        )
        .unwrap();

        let knowledge = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == card_id)
            .unwrap()
            .knowledge;
        assert!(knowledge.inferred_multicolor());
        // The most-recently-clued color is still tracked too.
        assert_eq!(knowledge.known_color, Some(Color::White));
    }

    #[test]
    fn cannot_clue_yourself() {
        let mut g = two_player_game();
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(0),
                clue: Clue::Number(1),
            },
        );
        assert_eq!(result.unwrap_err(), ActionError::CannotClueSelf);
    }

    #[test]
    fn playing_correct_card_advances_firework() {
        let mut g = two_player_game();
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::White,
            number: 1,
        };
        let card_id = g.hands[&PlayerId(0)][0].id;

        let events = g
            .apply_action(PlayerId(0), Action::Play { card_id })
            .unwrap();

        assert_eq!(g.fireworks[&Color::White], 1);
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::CardPlayed { success: true, .. })));
    }

    #[test]
    fn playing_wrong_card_costs_a_fuse() {
        let mut g = two_player_game();
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::White,
            number: 3,
        };
        let card_id = g.hands[&PlayerId(0)][0].id;

        g.apply_action(PlayerId(0), Action::Play { card_id }).unwrap();

        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS - 1);
        assert_eq!(g.fireworks[&Color::White], 0);
        assert!(g.discard_pile.iter().any(|c| *c
            == Card {
                color: Color::White,
                number: 3
            }));
    }

    #[test]
    fn cannot_discard_at_max_clue_tokens() {
        let mut g = two_player_game();
        let card_id = g.hands[&PlayerId(0)][0].id;
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);

        let result = g.apply_action(PlayerId(0), Action::Discard { card_id });
        assert_eq!(result.unwrap_err(), ActionError::CannotDiscardAtMaxClues);
    }

    #[test]
    fn discard_refunds_a_clue_token() {
        let mut g = two_player_game();
        let some_color = g.hands[&PlayerId(1)][0].card.color;
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(some_color),
            },
        )
        .unwrap();
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS - 1);

        let card_id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Discard { card_id })
            .unwrap();

        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
    }

    #[test]
    fn three_fuses_lost_ends_the_game() {
        let mut g = two_player_game();
        for _ in 0..3 {
            let player = g.current_player();
            // White-3 can never be a legal opening play (fireworks always
            // start at 0), so this reliably misses every time.
            g.hands.get_mut(&player).unwrap()[0].card = Card {
                color: Color::White,
                number: 3,
            };
            let card_id = g.hands[&player][0].id;
            g.apply_action(player, Action::Play { card_id }).unwrap();
        }

        assert_eq!(g.status, GameStatus::Finished(EndReason::FusesExhausted));
        assert_eq!(g.score(), 0);
    }

    #[test]
    fn last_move_is_recorded_for_clues_plays_and_discards() {
        let mut g = two_player_game();

        let color = g.hands[&PlayerId(1)][0].card.color;
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(color),
            },
        )
        .unwrap();
        match g.last_moves.get(&PlayerId(0)) {
            Some(LastMove::Clue { target, clue, .. }) => {
                assert_eq!(*target, PlayerId(1));
                assert_eq!(*clue, Clue::Color(color));
            }
            other => panic!("expected a recorded Clue move, got {other:?}"),
        }

        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::White,
            number: 1,
        };
        let card_id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id }).unwrap();
        match g.last_moves.get(&PlayerId(1)) {
            Some(LastMove::Play { card, success }) => {
                assert_eq!(
                    *card,
                    Card {
                        color: Color::White,
                        number: 1
                    }
                );
                assert!(success);
            }
            other => panic!("expected a recorded Play move, got {other:?}"),
        }

        let discard_id = g.hands[&PlayerId(0)][0].id;
        let discarded_card = g.hands[&PlayerId(0)][0].card;
        g.apply_action(PlayerId(0), Action::Discard { card_id: discard_id })
            .unwrap();
        match g.last_moves.get(&PlayerId(0)) {
            Some(LastMove::Discard { card }) => assert_eq!(*card, discarded_card),
            other => panic!("expected a recorded Discard move, got {other:?}"),
        }
    }

    #[test]
    fn not_your_turn_is_rejected() {
        let mut g = two_player_game();
        let card_id = g.hands[&PlayerId(1)][0].id;
        let result = g.apply_action(PlayerId(1), Action::Discard { card_id });
        assert_eq!(result.unwrap_err(), ActionError::NotYourTurn);
    }

    #[test]
    fn no_actions_allowed_once_game_over() {
        let mut g = two_player_game();
        for _ in 0..3 {
            let player = g.current_player();
            g.hands.get_mut(&player).unwrap()[0].card = Card {
                color: Color::White,
                number: 3,
            };
            let card_id = g.hands[&player][0].id;
            g.apply_action(player, Action::Play { card_id }).unwrap();
        }

        let player = g.current_player();
        let card_id = g.hands[&player][0].id;
        let result = g.apply_action(player, Action::Discard { card_id });
        assert_eq!(result.unwrap_err(), ActionError::GameOver);
    }
}
