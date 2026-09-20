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

/// `NUMBER_COUNTS` extended by one rank for `GameRules::six_cards`: the
/// previously-unique 5 becomes an ordinary pair, and the new 6 takes over
/// as the unique top card.
const NUMBER_COUNTS_SIX: [(u8, u8); 6] = [(1, 3), (2, 2), (3, 2), (4, 2), (5, 2), (6, 1)];

/// `REVERSE_NUMBER_COUNTS` extended by one rank for `GameRules::six_cards`.
/// This mirrors `NUMBER_COUNTS_SIX` rank-for-rank (three 6s down to one 1)
/// rather than just swapping 5 and 6 onto the old mirror: 6 becomes the
/// new "starting" rank a descending suit is built from, so it's the one
/// that's as common as 1 normally is, and 5 settles to an ordinary pair.
const REVERSE_NUMBER_COUNTS_SIX: [(u8, u8); 6] = [(1, 1), (2, 2), (3, 2), (4, 2), (5, 2), (6, 3)];

/// `SHORT_NUMBER_COUNTS` extended by one rank for `GameRules::six_cards`:
/// one copy of every rank 1-6, 6 cards total.
const SHORT_NUMBER_COUNTS_SIX: [(u8, u8); 6] = [(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1)];

/// Builds a deck for the given rules — 50 cards normally, plus 10 (5 if
/// that suit's "short" option is on, 12/6 instead if `six_cards` is also
/// on) more for each optional suit that's turned on (multicolor, black,
/// orange, purple). The hanabii mode is six colors of twelve cards each
/// (72 cards, 3/2/2/2/2/1 of ranks 1-6 per color).
pub fn standard_deck(rules: &GameRules) -> Vec<Card> {
    // The hanabii mode is a fixed preset (see `GameRules::normalized`), so
    // resolve it to the concrete options it stands for before reading any.
    let rules = rules.normalized();
    let colors = rules.active_colors();
    let mut deck = Vec::with_capacity(colors.len() * 10);
    for color in colors {
        let is_black = color == Color::Black;
        let counts: &[(u8, u8)] = match (rules.is_short(color), rules.six_cards, is_black) {
            (true, false, _) => &SHORT_NUMBER_COUNTS,
            (true, true, _) => &SHORT_NUMBER_COUNTS_SIX,
            (false, false, false) => &NUMBER_COUNTS,
            (false, false, true) => &REVERSE_NUMBER_COUNTS,
            (false, true, false) => &NUMBER_COUNTS_SIX,
            (false, true, true) => &REVERSE_NUMBER_COUNTS_SIX,
        };
        for &(number, count) in counts {
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
        let rules = GameRules { extra_colors: 2, ..Default::default() };
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
            extra_colors: 2,
            ..Default::default()
        };
        assert_eq!(standard_deck(&rules).len(), 90);
    }

    #[test]
    fn short_option_shrinks_a_suit_to_one_of_each_rank() {
        let rules = GameRules {
            multicolor: true,
            multicolor_short: true,
            extra_colors: 1,
            // extra_colors_short deliberately left off, for contrast
            ..Default::default()
        };
        let deck = standard_deck(&rules);

        let multi: Vec<_> = deck.iter().filter(|c| c.color == Color::Multicolor).collect();
        assert_eq!(multi.len(), 5);
        for rank in 1..=5 {
            assert_eq!(multi.iter().filter(|c| c.number == rank).count(), 1);
        }

        let purple: Vec<_> = deck.iter().filter(|c| c.color == Color::Purple).collect();
        assert_eq!(purple.len(), 10); // untouched: extra_colors_short is off
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
    fn six_cards_gives_a_normal_color_twelve_cards_with_a_unique_six() {
        let rules = GameRules { six_cards: true, ..Default::default() };
        let deck = standard_deck(&rules);
        for color in Color::ALL {
            let of_color: Vec<_> = deck.iter().filter(|c| c.color == color).collect();
            assert_eq!(of_color.len(), 12);
            assert_eq!(of_color.iter().filter(|c| c.number == 1).count(), 3);
            assert_eq!(of_color.iter().filter(|c| c.number == 5).count(), 2);
            assert_eq!(of_color.iter().filter(|c| c.number == 6).count(), 1);
        }
    }

    #[test]
    fn six_cards_gives_black_twelve_cards_mirrored_with_three_sixes() {
        let rules = GameRules { black: true, six_cards: true, ..Default::default() };
        let deck = standard_deck(&rules);
        let black: Vec<_> = deck.iter().filter(|c| c.color == Color::Black).collect();
        assert_eq!(black.len(), 12);
        assert_eq!(black.iter().filter(|c| c.number == 1).count(), 1);
        assert_eq!(black.iter().filter(|c| c.number == 2).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 3).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 4).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 5).count(), 2);
        assert_eq!(black.iter().filter(|c| c.number == 6).count(), 3);
    }

    #[test]
    fn six_cards_and_short_combine_to_one_of_each_rank_up_to_six() {
        let rules = GameRules {
            multicolor: true,
            multicolor_short: true,
            six_cards: true,
            ..Default::default()
        };
        let deck = standard_deck(&rules);
        let multi: Vec<_> = deck.iter().filter(|c| c.color == Color::Multicolor).collect();
        assert_eq!(multi.len(), 6);
        for rank in 1..=6 {
            assert_eq!(multi.iter().filter(|c| c.number == rank).count(), 1);
        }
    }

    #[test]
    fn six_cards_off_leaves_the_original_five_rank_distributions_untouched() {
        let rules = GameRules { black: true, six_cards: false, ..Default::default() };
        let deck = standard_deck(&rules);
        let black: Vec<_> = deck.iter().filter(|c| c.color == Color::Black).collect();
        assert_eq!(black.len(), 10);
        assert!(black.iter().all(|c| c.number <= 5));
    }

    #[test]
    fn hanabii_deck_is_six_colors_of_twelve_cards_with_the_six_card_distribution() {
        let rules = GameRules { hanabii: true, ..Default::default() };
        let deck = standard_deck(&rules);
        assert_eq!(deck.len(), 72);

        let colors = [
            Color::Red,
            Color::Orange,
            Color::Yellow,
            Color::Green,
            Color::Blue,
            Color::Purple,
        ];
        for color in colors {
            let of_color: Vec<_> = deck.iter().filter(|c| c.color == color).collect();
            assert_eq!(of_color.len(), 12, "{color:?}");
            // 3/1 2/2 2/3 2/4 2/5 1/6 — "count / rank".
            for (rank, count) in [(1, 3), (2, 2), (3, 2), (4, 2), (5, 2), (6, 1)] {
                assert_eq!(
                    of_color.iter().filter(|c| c.number == rank).count(),
                    count,
                    "{color:?} rank {rank}"
                );
            }
        }
        // Nothing but those six colors.
        assert!(deck.iter().all(|c| colors.contains(&c.color)));
    }

    #[test]
    fn hanabii_deck_ignores_every_other_option() {
        // Multicolor, black, short suits and a different extra-colors
        // count all switched on — none of it shows up in the deck.
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
        let plain = GameRules { hanabii: true, ..Default::default() };
        let mut a = standard_deck(&greedy);
        let mut b = standard_deck(&plain);
        a.sort_by_key(|c| (c.color as u8, c.number));
        b.sort_by_key(|c| (c.color as u8, c.number));
        assert_eq!(a, b);
    }

    #[test]
    fn hanabii_deck_matches_the_hand_built_extra_colors_one_six_cards_deck() {
        // "Until then it's just like six-card suits with six colors" — the
        // mode's deck is exactly that game's deck.
        let by_hand = GameRules { extra_colors: 1, six_cards: true, ..Default::default() };
        let mode = GameRules { hanabii: true, ..Default::default() };
        assert_eq!(standard_deck(&by_hand), standard_deck(&mode));
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
