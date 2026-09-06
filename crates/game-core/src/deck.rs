use crate::card::{Card, Color};

/// Standard Hanabi distribution per color: three 1s, two each of 2/3/4, one 5.
const NUMBER_COUNTS: [(u8, u8); 5] = [(1, 3), (2, 2), (3, 2), (4, 2), (5, 1)];

pub fn standard_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(50);
    for color in Color::ALL {
        for (number, count) in NUMBER_COUNTS {
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
pub fn shuffled_deck(seed: u64) -> Vec<Card> {
    let mut deck = standard_deck();

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

    #[test]
    fn standard_deck_has_fifty_cards() {
        assert_eq!(standard_deck().len(), 50);
    }

    #[test]
    fn each_color_has_ten_cards_with_one_five() {
        let deck = standard_deck();
        for color in Color::ALL {
            let of_color: Vec<_> = deck.iter().filter(|c| c.color == color).collect();
            assert_eq!(of_color.len(), 10);
            assert_eq!(of_color.iter().filter(|c| c.number == 5).count(), 1);
            assert_eq!(of_color.iter().filter(|c| c.number == 1).count(), 3);
        }
    }

    #[test]
    fn shuffle_is_deterministic_for_a_given_seed() {
        assert_eq!(shuffled_deck(1234), shuffled_deck(1234));
    }

    #[test]
    fn different_seeds_usually_differ() {
        assert_ne!(shuffled_deck(1), shuffled_deck(2));
    }

    #[test]
    fn shuffle_preserves_the_multiset_of_cards() {
        let mut original = standard_deck();
        let mut shuffled = shuffled_deck(99);
        original.sort_by_key(|c| (c.color as u8, c.number));
        shuffled.sort_by_key(|c| (c.color as u8, c.number));
        assert_eq!(original, shuffled);
    }
}
