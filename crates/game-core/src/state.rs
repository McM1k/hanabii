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
/// been played yet) and the game's `max_rank` (5, or 6 if
/// `GameRules::six_cards` is on). `None` once the suit is complete — the
/// frontend also uses that to tell "full for now" (e.g. a six-card suit
/// sitting at 5/6) apart from "actually done".
pub fn next_expected_rank(color: Color, top: Number, max_rank: Number) -> Option<Number> {
    if is_reverse_suit(color) {
        match top {
            0 => Some(max_rank),
            1 => None,
            n => Some(n - 1),
        }
    } else if top < max_rank {
        Some(top + 1)
    } else {
        None
    }
}

/// How many points a suit's firework is currently worth, given the rank of
/// whatever's on top (0 if nothing's been played) and the game's
/// `max_rank`. For a normal suit this is just the top rank; for a reverse
/// suit it's inverted, since a *lower* top rank means *more* cards have
/// been played — a complete reverse suit (top rank 1) is worth `max_rank`
/// points, same as a complete normal suit (top rank `max_rank`).
pub fn points_for(color: Color, top: Number, max_rank: Number) -> u8 {
    if is_reverse_suit(color) && top != 0 {
        max_rank + 1 - top
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
        /// Exactly which of the target's cards the clue touched — public
        /// information at a real table too, and what lets every player's
        /// screen briefly point at them. Empty for a hanabii-mode color
        /// clue that touched nothing.
        touched: Vec<CardId>,
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
    /// Hanabii mode only: a color clue must name a primary color (red,
    /// yellow or blue). The other colors are mixed from those, so they
    /// can't be named directly — they're reached through their ingredients.
    CannotClueSecondaryColor,
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
    /// Set the moment the last card is drawn: how many more turns will be
    /// played before the game ends — one for every player, *including the
    /// one who drew that last card*, who gets to play it (the standard
    /// rule). Counts down as those turns finish; the game is over when it
    /// reaches zero. `None` until the deck runs out.
    pub final_turns_remaining: Option<usize>,
    /// Who took the most recent turn, if anyone has yet. Together with
    /// `last_moves` this identifies "the latest move" for the UI.
    pub last_actor: Option<PlayerId>,
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
        // The hanabii mode is a fixed preset that replaces every other
        // option — resolve it once here so the stored rules (which are also
        // what every client is sent) are always the concrete ones played.
        let rules = rules.normalized();
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
            final_turns_remaining: None,
            last_actor: None,
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

        // Was the end-of-game countdown already running before this turn
        // began? (Measured up front because *this* turn may be the one that
        // draws the last card and starts it — that turn isn't one of the
        // final turns, it's the one that triggers them.)
        let countdown_already_running = self.final_turns_remaining.is_some();

        let mut events = match action {
            Action::Clue { target, clue } => self.apply_clue(player, target, clue)?,
            Action::Play { card_id } => self.apply_play(player, card_id)?,
            Action::Discard { card_id } => self.apply_discard(player, card_id)?,
        };

        self.last_actor = Some(player);
        self.advance_turn();

        if countdown_already_running {
            // One of the final turns just finished.
            if let Some(left) = self.final_turns_remaining.as_mut() {
                *left = left.saturating_sub(1);
            }
        }

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
            // `GameRules::color_clue_touches`) no color clue would touch it
            // anyway.
            return Err(ActionError::CannotClueBlack);
        }
        if self.rules.hanabii && matches!(clue, Clue::Color(c) if !c.is_primary()) {
            // Hanabii mode: only red, yellow and blue can be named. Orange,
            // green and purple are mixed from them, so they're only ever
            // touched *by* a primary clue, never named by one.
            return Err(ActionError::CannotClueSecondaryColor);
        }
        if self.clue_tokens == 0 {
            return Err(ActionError::NoClueTokens);
        }

        let rules = self.rules;
        let hand = self
            .hands
            .get_mut(&target)
            .expect("every seated player has a hand");

        // Which cards a clue touches is `GameRules::clue_touches`'s call —
        // own color plus wild multicolor cards in an ordinary game, every
        // color mixed with the named primary in hanabii mode, and Black
        // never — so the engine and the frontend's hover preview share one
        // definition.
        //
        // Worked out for the whole hand *before* anything is recorded: a
        // clue that touches nothing is rejected, and a rejected clue must
        // leave no trace (in particular, it mustn't hand the target free
        // negative information about their cards).
        let matches: Vec<bool> = hand
            .iter()
            .map(|hc| rules.clue_touches(clue, hc.card))
            .collect();
        let may_touch_nothing =
            matches!(clue, Clue::Color(_)) && rules.allows_empty_color_clues();
        if !may_touch_nothing && !matches.iter().any(|&is_match| is_match) {
            return Err(ActionError::ClueMatchesNothing);
        }

        let mut touched = Vec::new();
        for (hc, &is_match) in hand.iter_mut().zip(&matches) {
            if is_match {
                touched.push(hc.id);
            }
            hc.knowledge.apply_clue_result(clue, is_match, &rules);
        }

        self.clue_tokens -= 1;

        self.last_moves.insert(
            from,
            LastMove::Clue {
                target,
                clue,
                touched: touched.clone(),
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

        let max_rank = self.rules.max_rank();
        let top = *self.fireworks.get(&played.card.color).unwrap();
        let success = next_expected_rank(played.card.color, top, max_rank) == Some(played.card.number);

        if success {
            self.fireworks.insert(played.card.color, played.card.number);
            // Completing a firework refunds a clue token — for a reverse
            // suit that means finishing on a 1, not a 5 (or 6, with
            // `six_cards` on).
            let completed = next_expected_rank(played.card.color, played.card.number, max_rank).is_none();
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

            if self.draw_pile.is_empty() {
                self.start_final_turns();
            }

            vec![Event::CardDrawn { player, card_id: id }]
        } else {
            self.start_final_turns();
            vec![]
        }
    }

    /// The deck has run out: from here every player gets exactly one more
    /// turn, the player who drew the last card included (see
    /// `final_turns_remaining`). Does nothing if the countdown is already
    /// running.
    fn start_final_turns(&mut self) {
        if self.final_turns_remaining.is_none() {
            self.final_turns_remaining = Some(self.players.len());
        }
    }

    fn advance_turn(&mut self) {
        self.current_turn = (self.current_turn + 1) % self.players.len();
    }

    /// Checks and, if applicable, applies the game-over transition. Order
    /// matters: running out of fuses or completing every firework ends the
    /// game immediately, even mid final-round; otherwise the game ends once
    /// every player has had their one extra turn after the deck emptied —
    /// the last of which belongs to whoever drew the final card.
    fn check_game_over(&mut self) -> Option<Event> {
        if self.status != GameStatus::InProgress {
            return None;
        }

        let max_score = self.rules.max_score();
        if self.fuse_tokens == 0 {
            self.status = GameStatus::Finished(EndReason::FusesExhausted);
        } else if self.score() == max_score {
            self.status = GameStatus::Finished(EndReason::PerfectScore);
        } else if self.final_turns_remaining == Some(0) {
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
        let max_rank = self.rules.max_rank();
        self.fireworks
            .iter()
            .map(|(&color, &top)| points_for(color, top, max_rank))
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
            GameRules { extra_colors: 2, ..Default::default() },
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
    fn a_negative_clue_on_a_different_color_rules_out_multicolor_for_a_real_card() {
        let mut g = GameState::new(2, 42, GameRules { multicolor: true, black: false, ..Default::default() });
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card {
            color: Color::Red,
            number: 2,
        };
        let red_id = g.hands[&PlayerId(1)][0].id;
        // A real blue card elsewhere in the hand so the third clue below
        // actually touches something.
        g.hands.get_mut(&PlayerId(1)).unwrap()[1].card = Card {
            color: Color::Blue,
            number: 4,
        };
        // Pinned so the "pass the turn back" clue is guaranteed to touch
        // something, regardless of what the seed dealt.
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card {
            color: Color::White,
            number: 1,
        };

        // Turn 1: clue the red card about Red — one color matched so far,
        // so it's still ambiguous with multicolor.
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::Red),
            },
        )
        .unwrap();
        let knowledge_after_red_clue = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == red_id)
            .unwrap()
            .knowledge;
        assert!(knowledge_after_red_clue.could_be_multicolor(&g.rules));

        // Turn 2: pass the turn back.
        g.apply_action(
            PlayerId(1),
            Action::Clue {
                target: PlayerId(0),
                clue: Clue::Color(Color::White),
            },
        )
        .unwrap();

        // Turn 3: clue Blue — touches the *other* card, not the red one.
        // A multicolor card would have matched this too, so missing it
        // proves the red card really is just red.
        g.apply_action(
            PlayerId(0),
            Action::Clue {
                target: PlayerId(1),
                clue: Clue::Color(Color::Blue),
            },
        )
        .unwrap();

        let knowledge = &g.hands[&PlayerId(1)]
            .iter()
            .find(|hc| hc.id == red_id)
            .unwrap()
            .knowledge;
        assert!(
            !knowledge.could_be_multicolor(&g.rules),
            "a negative clue on another color rules out multicolor, even with only one color ever matched"
        );
        assert_eq!(knowledge.known_color, Some(Color::Red));
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
    fn six_cards_lets_a_normal_suit_play_past_five() {
        let mut g = GameState::new(2, 42, GameRules { six_cards: true, ..Default::default() });
        g.fireworks.insert(Color::Red, 5);

        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card { color: Color::Red, number: 6 };
        let red_six_id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: red_six_id })
            .unwrap();

        assert_eq!(*g.fireworks.get(&Color::Red).unwrap(), 6);
        assert_eq!(g.score(), 6);
        // The suit is complete now — no rank 7 to expect next.
        assert_eq!(next_expected_rank(Color::Red, 6, g.rules.max_rank()), None);
    }

    #[test]
    fn six_cards_makes_black_start_at_six_not_five() {
        let mut g = GameState::new(2, 42, GameRules { black: true, six_cards: true, ..Default::default() });
        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card { color: Color::Black, number: 5 };
        let black_five_id = g.hands[&PlayerId(0)][0].id;

        // A black 5 first should fail now — with six_cards on, Black
        // starts at 6, not 5.
        g.apply_action(PlayerId(0), Action::Play { card_id: black_five_id })
            .unwrap();
        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS - 1);
        assert_eq!(*g.fireworks.get(&Color::Black).unwrap(), 0);

        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card { color: Color::Black, number: 6 };
        let black_six_id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id: black_six_id })
            .unwrap();
        assert_eq!(*g.fireworks.get(&Color::Black).unwrap(), 6);
        assert_eq!(g.score(), 1); // one black card played, worth 1 point
    }

    #[test]
    fn six_cards_perfect_score_uses_six_per_suit() {
        let mut g = GameState::new(2, 42, GameRules { black: true, six_cards: true, ..Default::default() });
        for color in Color::ALL {
            g.fireworks.insert(color, 6);
        }
        g.fireworks.insert(Color::Black, 2); // one black play short of complete

        g.hands.get_mut(&PlayerId(0)).unwrap()[0].card = Card { color: Color::Black, number: 1 };
        let black_one_id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: black_one_id })
            .unwrap();

        assert_eq!(g.score(), 36); // 5 base suits at 6 each, plus black's 6
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

    // --- rejected clues ---------------------------------------------------

    #[test]
    fn a_clue_that_matches_nothing_is_rejected_without_leaving_a_trace() {
        // A rejected clue must not change anything — least of all what the
        // target's cards "know". Otherwise anyone could probe for free
        // (no token, no turn spent) and hand the target negative info that
        // a legal clue could never have given them.
        let mut g = two_player_game();
        for hc in g.hands.get_mut(&PlayerId(1)).unwrap().iter_mut() {
            hc.card = Card { color: Color::White, number: 2 };
        }
        let before: Vec<_> = g.hands[&PlayerId(1)].iter().map(|hc| hc.knowledge.clone()).collect();

        let result = g.apply_action(
            PlayerId(0),
            Action::Clue { target: PlayerId(1), clue: Clue::Color(Color::Red) },
        );
        assert_eq!(result.unwrap_err(), ActionError::ClueMatchesNothing);

        let after: Vec<_> = g.hands[&PlayerId(1)].iter().map(|hc| hc.knowledge.clone()).collect();
        assert_eq!(before, after, "a rejected clue changed the target's knowledge");
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
        assert_eq!(g.current_player(), PlayerId(0));
    }

    // --- hanabii mode -----------------------------------------------------

    fn hanabii_rules() -> GameRules {
        GameRules { hanabii: true, ..Default::default() }
    }

    fn hanabii_game() -> GameState {
        GameState::new(2, 42, hanabii_rules())
    }

    /// Replaces the first cards of a player's hand with the given colors
    /// (all rank 3 unless the ranks are given), returning their ids.
    fn set_hand(g: &mut GameState, player: PlayerId, cards: &[(Color, u8)]) -> Vec<CardId> {
        let hand = g.hands.get_mut(&player).unwrap();
        cards
            .iter()
            .enumerate()
            .map(|(i, &(color, number))| {
                hand[i].card = Card { color, number };
                hand[i].id
            })
            .collect()
    }

    fn touched_by(g: &mut GameState, from: PlayerId, target: PlayerId, clue: Clue) -> Vec<CardId> {
        let events = g.apply_action(from, Action::Clue { target, clue }).unwrap();
        match &events[0] {
            Event::ClueGiven { touched, .. } => touched.clone(),
            other => panic!("expected a ClueGiven event, got {other:?}"),
        }
    }

    #[test]
    fn hanabii_deals_from_a_seventy_two_card_deck_of_six_colors() {
        let g = GameState::new(3, 7, hanabii_rules());
        let dealt: usize = g.hands.values().map(|h| h.len()).sum();
        assert_eq!(dealt + g.draw_pile.len(), 72);

        let mut colors: Vec<Color> = g.fireworks.keys().copied().collect();
        colors.sort_by_key(|c| g.rules.active_colors().iter().position(|a| a == c));
        assert_eq!(
            colors,
            vec![Color::Red, Color::Orange, Color::Yellow, Color::Green, Color::Blue, Color::Purple]
        );
        assert!(g.fireworks.values().all(|&top| top == 0));
        assert_eq!(g.rules.max_score(), 36);
        assert_eq!(g.rules.max_rank(), 6);
    }

    #[test]
    fn hanabii_locks_the_other_options_when_the_game_starts() {
        // Every other toggle on — the game still starts as plain hanabii,
        // and the rules it stores (and sends to every client) say so.
        let greedy = GameRules {
            multicolor: true,
            black: true,
            extra_colors: 2,
            multicolor_short: true,
            black_short: true,
            extra_colors_short: true,
            hanabii: true,
            ..Default::default()
        };
        let g = GameState::new(2, 42, greedy);
        assert_eq!(g.rules, hanabii_rules().normalized());
        let dealt: usize = g.hands.values().map(|h| h.len()).sum();
        assert_eq!(dealt + g.draw_pile.len(), 72);
        assert_eq!(g.fireworks.len(), 6);
    }

    #[test]
    fn a_red_clue_touches_red_orange_and_purple_cards() {
        let mut g = hanabii_game();
        let ids = set_hand(
            &mut g,
            PlayerId(1),
            &[
                (Color::Red, 1),
                (Color::Orange, 2),
                (Color::Purple, 3),
                (Color::Yellow, 4),
                (Color::Green, 5),
            ],
        );

        let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Red));

        // Red, orange (red + yellow) and purple (blue + red) — but not
        // plain yellow, and not green (yellow + blue: no red in it).
        assert_eq!(touched, vec![ids[0], ids[1], ids[2]]);
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS - 1);
    }

    #[test]
    fn each_primary_clue_touches_exactly_the_colors_mixed_with_it() {
        let all = [
            Color::Red,
            Color::Orange,
            Color::Yellow,
            Color::Green,
            Color::Blue,
            Color::Purple,
        ];
        for (primary, expected) in [
            (Color::Red, vec![Color::Red, Color::Orange, Color::Purple]),
            (Color::Yellow, vec![Color::Orange, Color::Yellow, Color::Green]),
            (Color::Blue, vec![Color::Green, Color::Blue, Color::Purple]),
        ] {
            let mut g = hanabii_game();
            // Two players are dealt five cards each, so tack a sixth onto
            // player 1's hand to fit one card of every color.
            g.hands.get_mut(&PlayerId(1)).unwrap().push(HandCard {
                id: CardId(1000),
                card: Card { color: Color::Red, number: 1 },
                knowledge: CardKnowledge::default(),
            });
            let cards: Vec<(Color, u8)> = all.iter().map(|&color| (color, 1)).collect();
            let ids = set_hand(&mut g, PlayerId(1), &cards);

            let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(primary));
            let touched_colors: Vec<Color> = all
                .iter()
                .zip(&ids)
                .filter(|(_, id)| touched.contains(id))
                .map(|(&color, _)| color)
                .collect();
            assert_eq!(touched_colors, expected, "{primary:?} clue");
        }
    }

    #[test]
    fn only_primary_colors_can_be_named_in_a_hanabii_clue() {
        for color in [Color::Orange, Color::Green, Color::Purple, Color::White] {
            let mut g = hanabii_game();
            // A hand full of that very color, so the clue would certainly
            // touch something if it were allowed.
            set_hand(&mut g, PlayerId(1), &[(color, 1), (color, 2), (color, 3), (color, 4), (color, 5)]);

            let result = g.apply_action(
                PlayerId(0),
                Action::Clue { target: PlayerId(1), clue: Clue::Color(color) },
            );
            assert_eq!(result.unwrap_err(), ActionError::CannotClueSecondaryColor, "{color:?}");
            // Nothing happened: no token spent, still player 0's turn, and
            // nothing was taught to the target.
            assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
            assert_eq!(g.current_player(), PlayerId(0));
            assert!(g.hands[&PlayerId(1)]
                .iter()
                .all(|hc| hc.knowledge == CardKnowledge::default()));
        }
    }

    #[test]
    fn the_engine_accepts_exactly_the_colors_the_rules_say_are_cluable() {
        let every_color = [
            Color::White,
            Color::Red,
            Color::Yellow,
            Color::Green,
            Color::Blue,
            Color::Multicolor,
            Color::Black,
            Color::Orange,
            Color::Purple,
        ];
        let rules = hanabii_rules();
        let cluable = rules.cluable_colors();
        for color in every_color {
            let mut g = hanabii_game();
            // One card of every color in play, so a legal color clue can
            // never be turned away for touching nothing.
            set_hand(
                &mut g,
                PlayerId(1),
                &[
                    (Color::Red, 1),
                    (Color::Orange, 1),
                    (Color::Yellow, 1),
                    (Color::Green, 1),
                    (Color::Blue, 1),
                ],
            );
            let result = g.apply_action(
                PlayerId(0),
                Action::Clue { target: PlayerId(1), clue: Clue::Color(color) },
            );
            assert_eq!(result.is_ok(), cluable.contains(&color), "{color:?}: {result:?}");
        }
    }

    #[test]
    fn a_hanabii_color_clue_may_touch_nothing_and_still_teaches_the_target() {
        let mut g = hanabii_game();
        // Only green cards (yellow + blue): red touches none of them — but
        // in hanabii mode the primaries can always be given, because "no
        // red anywhere in your hand" is information too.
        let ids = set_hand(&mut g, PlayerId(1), &[(Color::Green, 1); 5]);
        let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Red));
        assert!(touched.is_empty());

        // It cost what any clue costs: a token and the turn.
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS - 1);
        assert_eq!(g.current_player(), PlayerId(1));

        // Every card was told "no red in you": red, orange and purple are
        // out, and yellow, green and blue are what's left.
        let rules = g.rules;
        for id in ids {
            let k = &g.hands[&PlayerId(1)].iter().find(|hc| hc.id == id).unwrap().knowledge;
            assert!(k.missed_primaries.contains(&Color::Red));
            assert!(k.hit_primaries.is_empty());
            assert_eq!(
                k.hanabii_possible_colors(&rules),
                vec![Color::Yellow, Color::Green, Color::Blue]
            );
        }

        // ...and the move is on record, touching nobody.
        match &g.last_moves[&PlayerId(0)] {
            LastMove::Clue { target, clue, touched } => {
                assert_eq!((*target, *clue), (PlayerId(1), Clue::Color(Color::Red)));
                assert!(touched.is_empty());
            }
            other => panic!("expected a clue, got {other:?}"),
        }
    }

    #[test]
    fn every_primary_can_be_given_to_any_hand_in_hanabii_mode() {
        // Whatever the hand holds, red, yellow and blue are all accepted.
        for hand_color in [Color::Red, Color::Orange, Color::Yellow, Color::Green, Color::Blue, Color::Purple] {
            for primary in Color::PRIMARIES {
                let mut g = hanabii_game();
                set_hand(&mut g, PlayerId(1), &[(hand_color, 1); 5]);
                let result = g.apply_action(
                    PlayerId(0),
                    Action::Clue { target: PlayerId(1), clue: Clue::Color(primary) },
                );
                assert!(result.is_ok(), "{primary:?} clue to a hand of {hand_color:?}: {result:?}");
            }
        }
    }

    #[test]
    fn an_empty_hanabii_clue_still_needs_a_clue_token() {
        let mut g = hanabii_game();
        set_hand(&mut g, PlayerId(1), &[(Color::Green, 1); 5]);
        g.clue_tokens = 0;
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue { target: PlayerId(1), clue: Clue::Color(Color::Red) },
        );
        assert_eq!(result.unwrap_err(), ActionError::NoClueTokens);
    }

    #[test]
    fn a_hanabii_number_clue_must_still_touch_a_card() {
        let mut g = hanabii_game();
        set_hand(&mut g, PlayerId(1), &[(Color::Green, 1); 5]);
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue { target: PlayerId(1), clue: Clue::Number(6) },
        );
        assert_eq!(result.unwrap_err(), ActionError::ClueMatchesNothing);
        // Rejected without a trace.
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
        assert_eq!(g.current_player(), PlayerId(0));
        assert!(g.hands[&PlayerId(1)].iter().all(|hc| hc.knowledge == CardKnowledge::default()));
    }

    #[test]
    fn ordinary_games_still_reject_a_color_clue_that_touches_nothing() {
        let mut g = two_player_game();
        for hc in g.hands.get_mut(&PlayerId(1)).unwrap().iter_mut() {
            hc.card = Card { color: Color::White, number: 2 };
        }
        let result = g.apply_action(
            PlayerId(0),
            Action::Clue { target: PlayerId(1), clue: Clue::Color(Color::Red) },
        );
        assert_eq!(result.unwrap_err(), ActionError::ClueMatchesNothing);
    }

    // --- the latest move, and which cards a clue touched -----------------

    #[test]
    fn a_clue_records_exactly_which_cards_it_touched_and_who_took_the_turn() {
        let mut g = two_player_game();
        assert_eq!(g.last_actor, None);
        let ids = set_hand(
            &mut g,
            PlayerId(1),
            &[
                (Color::Red, 1),
                (Color::Blue, 2),
                (Color::Red, 3),
                (Color::Green, 4),
                (Color::White, 5),
            ],
        );
        let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Red));
        assert_eq!(touched, vec![ids[0], ids[2]]);

        assert_eq!(g.last_actor, Some(PlayerId(0)));
        match &g.last_moves[&PlayerId(0)] {
            LastMove::Clue { touched: recorded, .. } => assert_eq!(recorded, &touched),
            other => panic!("expected a clue, got {other:?}"),
        }
        // Every player's view says the same thing.
        for viewer in [PlayerId(0), PlayerId(1)] {
            assert_eq!(g.view_for(viewer).last_actor, Some(PlayerId(0)));
        }
    }

    #[test]
    fn hanabii_clues_record_the_mixed_color_cards_they_touched_too() {
        let mut g = hanabii_game();
        let ids = set_hand(
            &mut g,
            PlayerId(1),
            &[
                (Color::Red, 1),
                (Color::Orange, 2),
                (Color::Purple, 3),
                (Color::Yellow, 4),
                (Color::Green, 5),
            ],
        );
        touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Red));
        match &g.last_moves[&PlayerId(0)] {
            LastMove::Clue { touched, .. } => assert_eq!(touched, &vec![ids[0], ids[1], ids[2]]),
            other => panic!("expected a clue, got {other:?}"),
        }
    }

    #[test]
    fn plays_and_discards_update_the_latest_actor_too() {
        let mut g = two_player_game();
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.last_actor, Some(PlayerId(0)));
        let id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.last_actor, Some(PlayerId(1)));
    }

    // --- the final turns after the deck runs out ---------------------------

    /// A legal clue for `from` to give `target`: a number one of the target's
    /// cards carries.
    fn some_number_clue(g: &GameState, target: PlayerId) -> Action {
        Action::Clue {
            target,
            clue: Clue::Number(g.hands[&target][0].card.number),
        }
    }

    fn last_drawn(events: &[Event]) -> CardId {
        events
            .iter()
            .find_map(|e| match e {
                Event::CardDrawn { card_id, .. } => Some(*card_id),
                _ => None,
            })
            .expect("the action should have drawn a card")
    }

    #[test]
    fn the_player_who_draws_the_last_card_gets_a_final_turn_to_play_it() {
        let mut g = two_player_game();
        g.fuse_tokens = 5; // room for a few misplays without ending the game
        g.draw_pile.truncate(1);
        assert_eq!(g.final_turns_remaining, None);

        // Player 0 plays a card and draws the very last one from the deck.
        let id = g.hands[&PlayerId(0)][0].id;
        let events = g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        let drawn = last_drawn(&events);
        assert!(g.draw_pile.is_empty());

        // That starts the countdown: one more turn for each of the two
        // players — player 1 first, then player 0 again. The turn that drew
        // the card doesn't count as one of them.
        assert_eq!(g.final_turns_remaining, Some(2));
        assert_eq!(g.status, GameStatus::InProgress);
        assert_eq!(g.current_player(), PlayerId(1));

        // Player 1's final turn.
        let clue = some_number_clue(&g, PlayerId(0));
        g.apply_action(PlayerId(1), clue).unwrap();
        assert_eq!(g.final_turns_remaining, Some(1));
        assert_eq!(g.status, GameStatus::InProgress);

        // It's player 0's turn again — and the card they just drew is
        // theirs to play.
        assert_eq!(g.current_player(), PlayerId(0));
        assert!(g.hands[&PlayerId(0)].iter().any(|hc| hc.id == drawn));
        g.apply_action(PlayerId(0), Action::Play { card_id: drawn }).unwrap();

        // Only now is the game over.
        assert_eq!(g.final_turns_remaining, Some(0));
        assert_eq!(g.status, GameStatus::Finished(EndReason::DeckExhausted));
        assert_eq!(
            g.apply_action(PlayerId(1), Action::Play { card_id: g.hands[&PlayerId(1)][0].id })
                .unwrap_err(),
            ActionError::GameOver
        );
    }

    #[test]
    fn the_final_turns_go_round_the_whole_table_ending_with_the_player_who_drew() {
        let mut g = GameState::new(3, 21, GameRules::default());
        g.fuse_tokens = 5;
        g.draw_pile.truncate(1);

        // Player 0 draws the last card.
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.final_turns_remaining, Some(3));

        // Players 1 and 2 each get one more turn...
        let clue = some_number_clue(&g, PlayerId(2));
        g.apply_action(PlayerId(1), clue).unwrap();
        assert_eq!((g.final_turns_remaining, g.status), (Some(2), GameStatus::InProgress));
        let clue = some_number_clue(&g, PlayerId(0));
        g.apply_action(PlayerId(2), clue).unwrap();
        assert_eq!((g.final_turns_remaining, g.status), (Some(1), GameStatus::InProgress));

        // ...and then it's back to player 0, whose turn is the last one.
        assert_eq!(g.current_player(), PlayerId(0));
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.status, GameStatus::Finished(EndReason::DeckExhausted));
    }

    #[test]
    fn the_countdown_starts_once_and_keeps_going_while_nobody_can_draw() {
        let mut g = two_player_game();
        g.fuse_tokens = 5;
        g.draw_pile.truncate(1);

        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.final_turns_remaining, Some(2));

        // Player 1 plays a card: there's nothing left to draw, which must
        // not restart the countdown.
        let id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.final_turns_remaining, Some(1));
        assert_eq!(g.hands[&PlayerId(1)].len(), 4, "no replacement card to draw");
    }

    #[test]
    fn no_countdown_runs_while_there_are_still_cards_to_draw() {
        let mut g = two_player_game();
        g.fuse_tokens = 5;
        assert!(g.draw_pile.len() > 2);
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.final_turns_remaining, None);
        // One card left after a draw: still not empty, still no countdown.
        g.draw_pile.truncate(2);
        let id = g.hands[&PlayerId(1)][0].id;
        g.apply_action(PlayerId(1), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.draw_pile.len(), 1);
        assert_eq!(g.final_turns_remaining, None);
    }

    #[test]
    fn running_out_of_fuses_still_ends_the_game_at_once_during_the_final_turns() {
        let mut g = two_player_game();
        g.draw_pile.truncate(1);
        g.fuse_tokens = 5;
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.final_turns_remaining, Some(2));

        // Player 1 misplays their last fuse: over immediately, without
        // waiting for player 0's final turn.
        g.fuse_tokens = 1;
        g.fireworks.insert(Color::White, 5); // nothing in hand can be played on top
        let id = g.hands[&PlayerId(1)][0].id;
        g.hands.get_mut(&PlayerId(1)).unwrap()[0].card = Card { color: Color::Red, number: 5 };
        g.apply_action(PlayerId(1), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.status, GameStatus::Finished(EndReason::FusesExhausted));
    }

    #[test]
    fn a_full_random_game_always_gives_the_drawer_of_the_last_card_a_final_turn() {
        // Plays whole games to the end of the deck with a deterministic
        // pseudo-random player and checks the shape of the ending every
        // time: the game only ends by deck exhaustion after exactly one
        // more turn for every seat, the last of which is the player who
        // drew the final card.
        let mut deck_endings = 0;
        for seed in 0..80u64 {
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5_5A5A_1234_4321;
            let mut next = move || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            };
            let players = 2 + (seed % 4) as u8;
            let mut g = GameState::new(players, seed, GameRules::default());
            g.fuse_tokens = 200; // fuses shouldn't be what ends these games
            let mut drawer: Option<PlayerId> = None;
            let mut turns_since_draw = 0;

            for _ in 0..500 {
                if g.status != GameStatus::InProgress {
                    break;
                }
                let me = g.current_player();
                let cards: Vec<CardId> = g.hands[&me].iter().map(|hc| hc.id).collect();
                // Mostly discards (they refund clues and never risk a fuse), some plays.
                let action = if next() % 4 == 0 || g.clue_tokens >= MAX_CLUE_TOKENS {
                    Action::Play { card_id: cards[(next() as usize) % cards.len()] }
                } else {
                    Action::Discard { card_id: cards[(next() as usize) % cards.len()] }
                };
                let was_counting = g.final_turns_remaining.is_some();
                let events = g.apply_action(me, action).unwrap();
                if !was_counting && g.final_turns_remaining.is_some() {
                    drawer = Some(me);
                    assert!(events.iter().any(|e| matches!(e, Event::CardDrawn { .. })));
                    assert!(g.draw_pile.is_empty());
                } else if was_counting {
                    turns_since_draw += 1;
                    if g.status == GameStatus::Finished(EndReason::DeckExhausted) {
                        assert_eq!(turns_since_draw, players as usize, "seed {seed}");
                        assert_eq!(Some(me), drawer, "seed {seed}: the drawer takes the very last turn");
                        deck_endings += 1;
                    } else {
                        assert!(turns_since_draw < players as usize, "seed {seed}");
                    }
                }
            }
        }
        assert!(deck_endings >= 40, "only {deck_endings} of 80 games ran the deck out");
    }

    #[test]
    fn number_clues_work_as_usual_in_hanabii_mode() {
        let mut g = hanabii_game();
        let ids = set_hand(
            &mut g,
            PlayerId(1),
            &[
                (Color::Red, 3),
                (Color::Orange, 3),
                (Color::Green, 4),
                (Color::Blue, 3),
                (Color::Purple, 6),
            ],
        );
        let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Number(3));
        assert_eq!(touched, vec![ids[0], ids[1], ids[3]]);
        // ...and it says nothing at all about color.
        let k = &g.hands[&PlayerId(1)][0].knowledge;
        assert_eq!(k.known_number, Some(3));
        assert_eq!(k.hanabii_possible_colors(&g.rules).len(), 6);
    }

    #[test]
    fn hanabii_clues_teach_the_target_about_primary_colors_not_the_card_color() {
        let mut g = hanabii_game();
        // The green card (index 3) is the one player 1 discards below.
        let ids = set_hand(
            &mut g,
            PlayerId(1),
            &[
                (Color::Orange, 1),
                (Color::Red, 2),
                (Color::Blue, 3),
                (Color::Green, 4),
                (Color::Purple, 5),
            ],
        );
        touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Red));

        let knowledge = |g: &GameState, id: CardId| {
            g.hands[&PlayerId(1)].iter().find(|hc| hc.id == id).unwrap().knowledge.clone()
        };
        let rules = g.rules;

        // Orange and red were hit: they could each be red, orange or
        // purple — and neither is claimed to *be* red.
        for id in [ids[0], ids[1], ids[4]] {
            let k = knowledge(&g, id);
            assert_eq!(k.known_color, None);
            assert_eq!(
                k.hanabii_possible_colors(&rules),
                vec![Color::Red, Color::Orange, Color::Purple]
            );
        }
        // Blue and green missed: neither can be red, orange or purple.
        for id in [ids[2], ids[3]] {
            let k = knowledge(&g, id);
            assert_eq!(
                k.ruled_out_colors(&rules),
                vec![Color::Red, Color::Orange, Color::Purple]
            );
        }

        // Player 1 throws a card away to hand the turn back (discarding
        // refunds a token, so this also keeps the clue budget healthy)...
        g.apply_action(PlayerId(1), Action::Discard { card_id: ids[3] }).unwrap();
        // ...and whatever was drawn to replace it is pinned to a blue card,
        // so it isn't a wildcard in the yellow clue that follows.
        g.hands.get_mut(&PlayerId(1)).unwrap().last_mut().unwrap().card =
            Card { color: Color::Blue, number: 1 };

        // Now yellow: it touches the orange card only.
        let touched = touched_by(&mut g, PlayerId(0), PlayerId(1), Clue::Color(Color::Yellow));
        assert_eq!(touched, vec![ids[0]]);

        // Red-and-yellow is orange, full stop.
        assert_eq!(knowledge(&g, ids[0]).hanabii_certain_color(&rules), Some(Color::Orange));
        // Hit by red, missed by yellow: red or purple.
        assert_eq!(
            knowledge(&g, ids[1]).hanabii_possible_colors(&rules),
            vec![Color::Red, Color::Purple]
        );
        // Missed by both red and yellow: it can only be blue.
        assert_eq!(knowledge(&g, ids[2]).hanabii_certain_color(&rules), Some(Color::Blue));
        // The ordinary bookkeeping stays untouched all the way through.
        for hc in &g.hands[&PlayerId(1)] {
            assert!(hc.knowledge.clued_colors.is_empty());
            assert!(hc.knowledge.not_colors.is_empty());
            assert_eq!(hc.knowledge.known_color, None);
            assert!(!hc.knowledge.inferred_multicolor());
        }
    }

    #[test]
    fn hanabii_fireworks_are_built_one_to_six_in_all_six_colors() {
        let mut g = hanabii_game();
        set_hand(&mut g, PlayerId(0), &[(Color::Orange, 1)]);
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.fireworks[&Color::Orange], 1);
        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS);

        // A 6 is a real, playable rank here: an orange firework at 5 takes
        // it, completes, and refunds a clue token.
        let mut g = hanabii_game();
        g.fireworks.insert(Color::Orange, 5);
        g.clue_tokens = MAX_CLUE_TOKENS - 1;
        set_hand(&mut g, PlayerId(0), &[(Color::Orange, 6)]);
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();
        assert_eq!(g.fireworks[&Color::Orange], 6);
        assert_eq!(g.clue_tokens, MAX_CLUE_TOKENS);
        assert_eq!(g.fuse_tokens, MAX_FUSE_TOKENS);
    }

    #[test]
    fn a_perfect_hanabii_game_scores_36() {
        let mut g = hanabii_game();
        for color in g.rules.active_colors() {
            g.fireworks.insert(color, 6);
        }
        g.fireworks.insert(Color::Purple, 5); // one play short of done

        set_hand(&mut g, PlayerId(0), &[(Color::Purple, 6)]);
        let id = g.hands[&PlayerId(0)][0].id;
        g.apply_action(PlayerId(0), Action::Play { card_id: id }).unwrap();

        assert_eq!(g.score(), 36);
        assert_eq!(g.status, GameStatus::Finished(EndReason::PerfectScore));
    }

    #[test]
    fn hanabii_knowledge_stays_sound_through_whole_random_games() {
        // Plays many complete games with a deterministic pseudo-random
        // player and, after every single move, checks the one thing that
        // must never be wrong: what a card's knowledge claims is
        // consistent with what the card really is. The true color is never
        // ruled out; a "certain" color is the real one; and none of the
        // ordinary color bookkeeping (which would misread composite hits
        // as multicolor) is ever touched.
        let rules = hanabii_rules().normalized();
        let mut clues_given = 0;
        let mut secondaries_pinned = 0;

        for seed in 0..60u64 {
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
            let mut next = move || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            };
            let players = 2 + (seed % 4) as u8;
            let mut g = GameState::new(players, seed, hanabii_rules());

            for _turn in 0..400 {
                if g.status != GameStatus::InProgress {
                    break;
                }
                let me = g.current_player();
                let others: Vec<PlayerId> = g.players.iter().copied().filter(|&p| p != me).collect();
                let my_cards: Vec<CardId> = g.hands[&me].iter().map(|hc| hc.id).collect();

                // Mostly clues, some discards, the odd play: enough of
                // each to reach the deep parts of the game without losing
                // to fuses in the first few turns.
                let mut accepted = false;
                for _attempt in 0..40 {
                    let roll = next() % 100;
                    let action = if roll < 65 {
                        let target = others[(next() as usize) % others.len()];
                        let clue = if next() % 3 == 0 {
                            Clue::Number(1 + (next() % 6) as u8)
                        } else {
                            // Deliberately draws from *every* color, so
                            // the engine's refusal of the non-primaries is
                            // exercised on live games too.
                            let all = [
                                Color::Red, Color::Orange, Color::Yellow,
                                Color::Green, Color::Blue, Color::Purple,
                            ];
                            Clue::Color(all[(next() as usize) % all.len()])
                        };
                        Action::Clue { target, clue }
                    } else if roll < 90 {
                        Action::Discard { card_id: my_cards[(next() as usize) % my_cards.len()] }
                    } else {
                        Action::Play { card_id: my_cards[(next() as usize) % my_cards.len()] }
                    };
                    let is_clue = matches!(action, Action::Clue { .. });
                    if g.apply_action(me, action).is_ok() {
                        accepted = true;
                        if is_clue {
                            clues_given += 1;
                        }
                        break;
                    }
                }
                if !accepted {
                    // Always possible: playing a card never errors.
                    g.apply_action(me, Action::Play { card_id: my_cards[0] }).unwrap();
                }

                for hand in g.hands.values() {
                    for hc in hand {
                        let k = &hc.knowledge;
                        assert!(
                            k.could_be_hanabii_color(hc.card.color),
                            "seed {seed}: {:?} was ruled out for a {:?} card",
                            k.hanabii_possible_colors(&rules),
                            hc.card.color
                        );
                        if let Some(certain) = k.hanabii_certain_color(&rules) {
                            assert_eq!(certain, hc.card.color, "seed {seed}");
                            if !hc.card.color.is_primary() {
                                secondaries_pinned += 1;
                            }
                        }
                        assert_eq!(k.known_color, None, "seed {seed}");
                        assert!(k.clued_colors.is_empty() && k.not_colors.is_empty(), "seed {seed}");
                        assert!(!k.inferred_multicolor(), "seed {seed}");
                    }
                }
            }
        }
        // Guards the test itself: it really did exercise clues, and really
        // did pin down orange/green/purple cards along the way.
        assert!(clues_given > 500, "only {clues_given} clues were given");
        assert!(secondaries_pinned > 50, "only {secondaries_pinned} secondary cards were pinned");
    }
}
