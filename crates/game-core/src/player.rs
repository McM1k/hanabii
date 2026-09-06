use serde::{Deserialize, Serialize};

/// A seat at the table, 0-indexed in turn order. The server maps these onto
/// actual network connections; game-core doesn't know or care about that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlayerId(pub u8);
