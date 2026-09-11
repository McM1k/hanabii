use crate::card::{Card, Color};
use crate::rules::GameRules;

/// Standard Hanabi distribution per color: three 1s, two each of 2/3/4, one 5.
const NUMBER_COUNTS: [(u8, u8); 5] = [(1, 3), (2, 2), (3, 2), (4, 2), (5, 1)];

/// Same total (10 cards) and shape as the standard distribution, but
/// mirrored onto the ranks: three 5s down to one 1. Used for suits that
/// build their firework in descending order (currently just Black), where
/// 5 is the "starting" rank and should be as common as 1 normally is.
const REVERSE_NUMBER_COUNTS: [(u8, u8); 5] = [(1, 1), (2, 2), (3, 2), (4, 2), (5, 3)];

/// The "short" distribution any optional suit can use instead of its usual
/// one (see `GameRules::is_short`): one copy of every rank, 5 cards total,
/// every one of them irreplaceable. Direction doesn't matter here — with a
/// single copy of each rank, the ascending and mirrored-descending
/// distributions are identical, so this one template covers both.
const SHORT_NUMBER_COUNTS: [(u8, u8); 5] = [(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)];

/// Builds a deck for the given rules — 50 cards normally, plus 10 (or 5, if
/// that suit's "short" option is on) more for each optional suit that's
/// turned on (multicolor, black, orange, purple).
pub fn standard_deck(rules: &GameRules) -> Vec<Card> {
    let colors = rules.active_colors();
    let mut deck = Vec::with_capacity(colors.len() * 10);
    for color in colors {
        let counts = if rules.is_short(color) {
            SHORT_NUMBER_COUNTS
        } else if color == Color::Black {
            REVERSE_NUMBER_COUNTS
        } else {
            NUMBER_COUNTS
        };
        for (number, count) in counts {
            for _ in 0..count {
                deck.push(Card { color, number });
            }
        }
    }
    deck
}

/// Deterministic Fisher-Yates shuffle from a seed. Deliberately hand-rolled
/// instead of pulling in the `rand` crate: it's a handful of lines, has no
/// dependency on OS entropy (which WASM doesn't have without extra setup),
/// and this crate compiles for both the server and the wasm32 frontend.
/// Game logic only ever needs *a* shuffle to be deterministic given a seed —
/// the server is responsible for picking that seed randomly per game.
pub fn shuffled_deck(seed: u64, rules: &GameRules) -> Vec<Card> {
    let mut deck = standard_deck(rules);

    // xorshift64*, seeded away from zero so seed = 0 doesn't degenerate.
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next_u64 = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    for i in (1..deck.len()).rev() {
        let j = (next_u64() % (i as u64 + 1)) as usize;
        deck.swap(i, j);
    }

    deck
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Color;

    #[test]
    fn standard_deck_has_fifty_cards() {
        assert_eq!(standard_deck(&GameRules::default()).len(), 50);
    }

    #[test]
    fn each_color_has_ten_cards_with_one_five() {
        let deck = standard_deck(&GameRules::default());
        for color in Color::ALL {
            let of_color: Vec<_> = deck.iter().filter(|c| c.color == color).collect();
            assert_eq!(of_color.len(), 10);
            assert_eq!(of_color.iter().filter(|c| c.number == 5).count(), 1);
            assert_eq!(of_color.iter().filter(|c| c.number == 1).count(), 3);
        }
    }

    #[test]
    fn multicolor_rule_adds_a_sixth_ten_card_suit() {
        let rules = GameRules { multicolor: true, black: false, ..Default::default() };
        let deck = standard_deck(&rules);
        assert_eq!(deck.len(), 60);
        let multi: Vec<_> = deck.iter().filter(|c| c.color == Color::Multicolor).collect();
        assert_eq!(multi.len(), 10);
        assert_eq!(multi.iter().filter(|c| c.number == 5).count(), 1);
    }

    #[test]
    fn black_rule_adds_a_mirrored_ten_card_suit() {
        let rules = GameRules { multicolor: false, black: true, ..Default::default() };
        let deck = standard_deck(&rules);
        assert_eq!(deck.len(), 60);
        let black: Vec<_> = deck.iter().filter(|c| c.color == Color::Black).collect();
        assert_eq!(black.len(), 10);
        // Mirrored: three 5s (the "starting" rank), one 1 (the "finishing" rank).
        assert_eq!(black.iter().filter(|c| c.number == 5).count(), 3);
        assert_eq!(black.iter().filter(|c| c.number == 1).count(), 1);
        assert_eq!(black.iter().filter(|c| c.number == 2).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 3).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 4).count(), 2);
    }

    #[test]
    fn both_optional_suits_stack_to_seventy_cards() {
        let rules = GameRules { multicolor: true, black: true, ..Default::default() };
        assert_eq!(standard_deck(&rules).len(), 70);
    }

    #[test]
    fn orange_and_purple_are_plain_ten_card_suits() {
        let rules = GameRules { orange: true, purple: true, ..Default::default() };
        let deck = standard_deck(&rules);
        assert_eq!(deck.len(), 70);
        for color in [Color::Orange, Color::Purple] {
            let of_color: Vec<_> = deck.iter().filter(|c| c.color == color).collect();
            assert_eq!(of_color.len(), 10);
            assert_eq!(of_color.iter().filter(|c| c.number == 5).count(), 1);
            assert_eq!(of_color.iter().filter(|c| c.number == 1).count(), 3);
        }
    }

    #[test]
    fn all_four_optional_suits_stack_to_ninety_cards() {
        let rules = GameRules {
            multicolor: true,
            black: true,
            orange: true,
            purple: true,
            ..Default::default()
        };
        assert_eq!(standard_deck(&rules).len(), 90);
    }

    #[test]
    fn short_option_shrinks_a_suit_to_one_of_each_rank() {
        let rules = GameRules {
            multicolor: true,
            multicolor_short: true,
            orange: true,
            // orange_short deliberately left off, for contrast
            ..Default::default()
        };
        let deck = standard_deck(&rules);

        let multi: Vec<_> = deck.iter().filter(|c| c.color == Color::Multicolor).collect();
        assert_eq!(multi.len(), 5);
        for rank in 1..=5 {
            assert_eq!(multi.iter().filter(|c| c.number == rank).count(), 1);
        }

        let orange: Vec<_> = deck.iter().filter(|c| c.color == Color::Orange).collect();
        assert_eq!(orange.len(), 10); // untouched: its own short flag is off
    }

    #[test]
    fn short_black_is_still_five_cards_one_of_each_rank() {
        let rules = GameRules { black: true, black_short: true, ..Default::default() };
        let deck = standard_deck(&rules);
        let black: Vec<_> = deck.iter().filter(|c| c.color == Color::Black).collect();
        assert_eq!(black.len(), 5);
        for rank in 1..=5 {
            assert_eq!(black.iter().filter(|c| c.number == rank).count(), 1);
        }
    }

    #[test]
    fn shuffle_is_deterministic_for_a_given_seed() {
        let rules = GameRules::default();
        assert_eq!(shuffled_deck(1234, &rules), shuffled_deck(1234, &rules));
    }

    #[test]
    fn different_seeds_usually_differ() {
        let rules = GameRules::default();
        assert_ne!(shuffled_deck(1, &rules), shuffled_deck(2, &rules));
    }

    #[test]
    fn shuffle_preserves_the_multiset_of_cards() {
        let rules = GameRules::default();
        let mut original = standard_deck(&rules);
        let mut shuffled = shuffled_deck(99, &rules);
        original.sort_by_key(|c| (c.color as u8, c.number));
        shuffled.sort_by_key(|c| (c.color as u8, c.number));
        assert_eq!(original, shuffled);
    }
}
