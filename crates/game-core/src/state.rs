use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::card::{Card, CardId, Clue, Color, Number};
use crate::deck::shuffled_deck;
use crate::knowledge::CardKnowledge;
use crate::player::PlayerId;
use crate::rules::GameRules;

pub const MAX_CLUE_TOKENS: u8 = 8;
pub const MAX_FUSE_TOKENS: u8 = 3;

/// True for suits that build their firework in descending order (5 down to
/// 1) instead of the usual ascending 1-to-5 — currently just Black.
fn is_reverse_suit(color: Color) -> bool {
    color == Color::Black
}

/// The rank that would need to be played next to keep building this suit's
/// firework, given the rank of whatever's currently on top (0 if nothing's
/// been played yet). `None` once the suit is complete.
fn next_expected_rank(color: Color, top: Number) -> Option<Number> {
    if is_reverse_suit(color) {
        match top {
            0 => Some(5),
            1 => None,
            n => Some(n - 1),
        }
    } else if top < 5 {
        Some(top + 1)
    } else {
        None
    }
}

/// How many points a suit's firework is currently worth, given the rank of
/// whatever's on top (0 if nothing's been played). For a normal suit this
/// is just the top rank; for a reverse suit it's inverted, since a *lower*
/// top rank means *more* cards have been played.
fn points_for(color: Color, top: Number) -> u8 {
    if is_reverse_suit(color) && top != 0 {
        6 - top
    } else {
        top
    }
}

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
    /// The black suit has no color at all for clue purposes — it can't be
    /// named in a clue any more than it can be touched by one.
    CannotClueBlack,
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
        if matches!(clue, Clue::Color(Color::Black)) {
            // Black has no color at all — nothing to name it with, and (see
            // the is_match arm below) no color clue would touch it anyway.
            return Err(ActionError::CannotClueBlack);
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
                // any multicolor cards in the hand. Black cards never match
                // a color clue at all — that falls out of this check for
                // free, since Black can only ever equal itself, and `c` is
                // never Black or Multicolor (both rejected above).
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
        let success = next_expected_rank(played.card.color, top) == Some(played.card.number);

        if success {
            self.fireworks.insert(played.card.color, played.card.number);
            // Completing a firework refunds a clue token — for a reverse
            // suit that means finishing on a 1, not a 5.
            let completed = next_expected_rank(played.card.color, played.card.number).is_none();
            if completed && self.clue_tokens < MAX_CLUE_TOKENS {
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

        let max_score = self.rules.active_colors().len() as u8 * 5;
        if self.fuse_tokens == 0 {
            self.status = GameStatus::Finished(EndReason::FusesExhausted);
        } else if self.score() == max_score {
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

    /// Total points across every firework. For a normal suit the top rank
    /// *is* the point count; a reverse suit (Black) is inverted, since it
    /// counts down rather than up — see `points_for`.
    pub fn score(&self) -> u8 {
        self.fireworks
            .iter()
            .map(|(&color, &top)| points_for(color, top))
            .sum()
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
        let g = GameState::new(3, 7, GameRules { multicolor: true, black: false, ..Default::default() });
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
        let mut g = GameState::new(2, 42, GameRules { multicolor: true, black: false, ..Default::default() });
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
    fn orange_and_purple_behave_like_any_other_suit() {
        let mut g = GameState::new(
            2,
            42,
            GameRules { orange: true, purple: true, ..Default::default() },
        );
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Orange,
            number: 1,
        };
        let orange_id = g.hands[&PlayerId(1)][0].id;

        // Clued normally, exactly like a base color — no wildcard, no
        // "can't be named directly" restriction.
        let events = g
            .apply_action(
                PlayerId(0),
                Action::Clue {
                    target: PlayerId(1),
                    clue: Clue::Color(Color::Orange),
                },
            )
            .unwrap();
        match &events[0] {
            Event::ClueGiven { touched, .. } => assert!(touched.contains(&orange_id)),
            other => panic!("expected a ClueGiven event, got {other:?}"),
        }

        // Played in normal ascending order, starting at 1 — not reversed
        // like black.
        g.apply_action(PlayerId(1), Action::Play { card_id: orange_id })
            .unwrap();
        assert_eq!(*g.fireworks.get(&Color::Orange).unwrap(), 1);
    }

    #[test]
    fn cannot_clue_multicolor_directly() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: true, black: false, ..Default::default() });
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
        let mut g = GameState::new(2, 42, GameRules { multicolor: true, black: false, ..Default::default() });
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
    fn cannot_clue_black_directly() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::Black),
            },
        );
        assert_eq!(result.unwrap_err(), ActionError::CannotClueBlack);
    }

    #[test]
    fn ruling_out_every_base_color_reveals_a_card_as_black_by_elimination() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 3,
        };
        let black_id = g.hands[&PlayerId(1)][0].id;

        let colors_to_rule_out = [Color::White, Color::Red, Color::Yellow, Color::Green, Color::Blue];
        for (i, &color) in colors_to_rule_out.iter().enumerate() {
            // A throwaway second card in the same hand, recolored each
            // round to match whatever's being clued, purely so the clue
            // actually touches *something* and isn't rejected as
            // ClueMatchesNothing.
            g.hands.get_mut(&PlayerId(1)).unwrap()[1].card = Card { color, number: 2 };

            g.apply_action(
                PlayerId(0),
                Action::Clue {
                    target: PlayerId(1),
                    clue: Clue::Color(color),
                },
            )
            .unwrap();

            let knowledge = &g.hands[&PlayerId(1)]
                .iter()
                .find(|hc| hc.id == black_id)
                .unwrap()
                .knowledge;
            let is_last = i == colors_to_rule_out.len() - 1;
            assert_eq!(
                knowledge.inferred_black(&g.rules),
                is_last,
                "after ruling out {} of 5 colors",
                i + 1
            );

            if !is_last {
                // Pass the turn back via a discard rather than another
                // clue — discarding refunds a token instead of spending
                // one, so 5 rounds of "clue P1, pass back" don't run the
                // 8-token budget dry. Discards from index 2, well clear of
                // the black card (0) and the recolored helper (1).
                let discard_id = g.hands[&PlayerId(1)][2].id;
                g.apply_action(PlayerId(1), Action::Discard { card_id: discard_id })
                    .unwrap();
            }
        }
    }

    #[test]
    fn color_clues_never_touch_black_cards() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 3,
        };
        // A real red card too, so the clue below actually touches
        // *something* and isn't rejected as ClueMatchesNothing.
        g.hands.get_mut(&PlayerId(1)).unwrap()[1].card = Card {
            color: Color::Red,
            number: 2,
        };
        let black_id = g.hands[&PlayerId(1)][0].id;

        let events = g
            .apply_action(
                PlayerId(0),
                Action::Clue {
                    target: PlayerId(1),
                    clue: Clue::Color(Color::Red),
                },
            )
            .unwrap();
        match &events[0] {
            Event::ClueGiven { touched, .. } => assert!(!touched.contains(&black_id)),
            other => panic!("expected a ClueGiven event, got {other:?}"),
        }
        let knowledge = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == black_id)
            .unwrap()
            .knowledge;
        // Not being touched teaches the same negative info as any other
        // non-matching card.
        assert!(knowledge.not_colors.contains(&Color::Red));
    }

    #[test]
    fn black_suit_must_be_played_in_descending_order() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 1,
        };
        let black_one_id = g.hands[&PlayerId(0)][0].id;

        // Playing the 1 first should fail — Black starts at 5, not 1.
        g.apply_action(PlayerId(0), Action::Play { card_id: black_one_id })
            .unwrap();
        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS - 1);
        assert_eq!(*g.fireworks.get(&Color::Black).unwrap(), 0);

        // A black 5, played next, should succeed.
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 5,
        };
        let black_five_id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id: black_five_id })
            .unwrap();
        assert_eq!(*g.fireworks.get(&Color::Black).unwrap(), 5);
        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS - 1); // unchanged: this one succeeded
    }

    #[test]
    fn completing_black_suit_refunds_a_clue_token() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        // Fast-forward to 5,4,3,2 already played, with a token spent so a
        // refund is actually observable.
        g.fireworks.insert(Color::Black, 2);
        g.clue_tokens = MAX_CLUE_TOKENS - 1;

        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 1,
        };
        let black_one_id = g.hands[&PlayerId(0)][0].id;

        g.apply_action(PlayerId(0), Action::Play { card_id: black_one_id })
            .unwrap();

        assert_eq!(*g.fireworks.get(&Color::Black).unwrap(), 1);
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
    }

    #[test]
    fn score_counts_black_progress_correctly_despite_descending_ranks() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        // Two black cards played (5 then 4) is 2 points, even though the
        // rank sitting on top of the pile (4) is *lower* than the count
        // would suggest for a normal ascending suit.
        g.fireworks.insert(Color::Black, 4);
        assert_eq!(g.score(), 2);
    }

    #[test]
    fn perfect_score_with_black_uses_the_right_max() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: false, black: true, ..Default::default() });
        for color in Color::ALL {
            g.fireworks.insert(color, 5);
        }
        g.fireworks.insert(Color::Black, 2); // one black play short of complete

        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 1,
        };
        let black_one_id = g.hands[&PlayerId(0)][0].id;

        g.apply_action(PlayerId(0), Action::Play { card_id: black_one_id })
            .unwrap();

        assert_eq!(g.score(), 30); // 5 base suits at 5 each, plus black's 5
        assert_eq!(g.status, GameStatus::Finished(EndReason::PerfectScore));
    }

    #[test]
    fn both_optional_suits_together_give_seventy_cards_and_max_score_35() {
        // Deck/deal size: 5 base suits + multicolor + black, 10 cards each.
        let g = GameState::new(3, 7, GameRules { multicolor: true, black: true, ..Default::default() });
        let dealt: usize = g.hands.values().map(|h| h.len()).sum();
        assert_eq!(dealt + g.draw_pile.len(), 70);
        assert_eq!(g.fireworks.len(), 7);

        // Perfect-score check uses the right max (35) when both are on —
        // exercised end-to-end through a real play, not just computed.
        let mut g = GameState::new(2, 42, GameRules { multicolor: true, black: true, ..Default::default() });
        for color in Color::ALL {
            g.fireworks.insert(color, 5);
        }
        g.fireworks.insert(Color::Multicolor, 5);
        g.fireworks.insert(Color::Black, 2); // one black play short of complete

        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::Black,
            number: 1,
        };
        let black_one_id = g.hands[&PlayerId(0)][0].id;

        g.apply_action(PlayerId(0), Action::Play { card_id: black_one_id })
            .unwrap();

        assert_eq!(g.score(), 35); // (5 base + multicolor) * 5, plus black's 5
        assert_eq!(g.status, GameStatus::Finished(EndReason::PerfectScore));
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
