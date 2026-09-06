use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Color {
    White,
    Red,
    Yellow,
    Green,
    Blue,
}

impl Color {
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Red,
        Color::Yellow,
        Color::Green,
        Color::Blue,
    ];
}

/// Card ranks run 1-5. A plain alias keeps this easy to serialize and compare;
/// nothing here enforces the range at the type level, the engine does that.
pub type Number = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Card {
    pub color: Color,
    pub number: Number,
}

/// Stable identity for one physical card for the whole game, independent of
/// which hand slot it's currently sitting in. This is how a client tracks
/// "the card I was told is red" even while its number is still unknown, and
/// how actions reference a specific card without needing hand positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardId(pub u32);

/// The two things you're allowed to clue about a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Clue {
    Color(Color),
    Number(Number),
}
