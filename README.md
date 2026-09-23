# Hanabi (working title)

A multiplayer clone of the cooperative card game Hanabi, built in Rust end-to-end:
Axum for the server, Leptos (WASM) for the frontend, WebSockets tying them together.

## Status

- [x] `game-core` — the rules engine. Compiler-verified, 119/119 tests passing.
- [x] `server` — Axum + WebSockets, room management. Compiler-verified, 12/12 tests
      passing, join flow tested manually.
- [x] `frontend` — Leptos UI. Compiler-verified every round (zero errors, zero
      warnings) via a sandbox-only dependency-pinning workaround (see
      `crates/frontend/Cargo.toml` — never touch the pinned versions there,
      they're the shippable ones; the workaround happens transiently during
      Claude's own verification, not in the committed file).
      **A real regression happened**: the fireworks row got a resize-based
      "balance tiles evenly across lines" feature whose `NodeRef` was attached
      to a DOM node living inside a closure that fully rebuilds on every game
      state update. A `create_effect` watching that ref reactively for "has it
      mounted" refired on every one of those rebuilds, feeding a signal the
      same closure read — a reactive loop. A first attempt to fix this forward
      (gating the effect so it only *acts* once) did not resolve the reported
      symptoms (drag-and-drop broken, plus a new one: clue-target selection
      broken) and things got worse, not better. Rather than keep guessing
      blind against something Claude cannot see run in a real browser, the
      entire feature was removed and the fireworks row reverted to plain CSS
      `flex-wrap` (no balancing, just greedy wrap — the behavior from many
      rounds before this one, confirmed working then). If drag-and-drop or
      clue-target selection are still broken after this revert, the cause is
      something else, and the fastest way to find it is the browser console's
      actual error output — Claude cannot reliably diagnose further from
      source-reading alone. The general lesson for any future DOM-measurement
      feature: a `NodeRef`/signal pairing where the ref's owning element lives
      inside a closure that rebuilds in response to the *same* signal is a
      reactive-loop risk.

## Running everything

Three separate processes, three separate commands — see "Why three separate
build commands" below for why this isn't just one `cargo build`.

```bash
# 1. the rules engine — already confirmed passing
cargo test -p game-core

# 2. the server — already confirmed building and running
cargo run -p server
# listens on ws://0.0.0.0:3000/ws

# 3. the frontend — NOT yet verified, try this next
rustup target add wasm32-unknown-unknown   # one-time
cargo install trunk                         # one-time
cd crates/frontend
trunk serve
# opens on http://127.0.0.1:8080 by default
```

With the server running in one terminal and `trunk serve` in another, open
`http://127.0.0.1:8080` in two browser tabs/windows, join the same room code
with two different names from each, and start the game once both have joined.

If `trunk serve` fails to compile, paste the error back. Given the API
uncertainty noted above, this is the crate most likely to need fixes.

## Why three separate build commands

`frontend` depends on wasm-only crates (`gloo-net`, `wasm-bindgen-futures`)
that won't compile for a native target. A plain `cargo build` at the workspace
root (no `-p`) will try to build every member for the host target and fail on
`frontend` for that reason — this is expected, not a bug. Building each crate
with an explicit `-p` (or using `trunk` for the frontend specifically, which
handles the `wasm32-unknown-unknown` target itself) avoids it.

## Project layout

```
hanabi/
  Cargo.toml                  # workspace root
  crates/
    game-core/                # pure game rules — no networking, no UI
      src/
        card.rs                # Color, Card, CardId, Clue
        player.rs               # PlayerId
        deck.rs                  # standard 50-card deck + seeded shuffle
        knowledge.rs              # what a player has been told about their own cards
        state.rs                   # GameState + the rule engine (apply_action)
        protocol.rs                 # PlayerView — the redacted view sent to each client
        messages.rs                  # ClientMessage / ServerMessage — the wire protocol
    server/                    # Axum + WebSockets
      src/
        main.rs                 # app setup, shared state, routing
        ws.rs                    # the WebSocket connection lifecycle
        room.rs                   # room/seat bookkeeping, wraps a GameState
    frontend/                  # Leptos (CSR, via Trunk)
      index.html                # trunk entry point, also loads Fraunces/Inter
      style.css                  # night-sky palette, cards rendered as actual card tiles
      src/
        main.rs                   # mounts the App
        ws.rs                      # AppContext (shared reactive state) + connect()
        app.rs                      # routes between Joining / Lobby / Playing
        screens.rs                   # JoinScreen, Lobby
        game_board.rs                 # the game itself — fireworks, hands, actions
```

## Design notes

- **No `rand` crate in `game-core`.** Shuffling uses a small hand-rolled seeded PRNG
  (xorshift64*) instead of a dependency, so it stays at zero dependencies beyond `serde`
  and compiles cleanly to `wasm32-unknown-unknown`.
- **Card identity vs. card value.** Every card gets a `CardId`, stable for the whole game.
  Actions reference cards by `CardId`, not hand position, since a player doesn't always
  know their own cards' values but can still refer to "my leftmost card."
- **Redaction lives in `game-core`, not the server.** `GameState::view_for(player)` is a
  pure, unit-tested function; the server's job is just to call it and route the result.
- **Full state resync over granular deltas.** Every accepted action triggers a fresh
  `PlayerView` push to each seated player, rather than streaming individual events.
- **One room = one `std::sync::Mutex<Room>`,** no async mutex needed — every operation
  touching room state is synchronous internally, so the lock is always released before
  any `.await`.
- **Key dependencies pinned, not left open**: `axum = "0.7"`, `leptos = "0.6"`. My
  knowledge of their APIs only reliably extends to early 2026, so pinning means they
  resolve to versions I actually know rather than whatever's newest.
- **Play and discard are drag-and-drop only** — drag a card from your hand onto the
  fireworks panel to play it, or onto the discard pile to discard it. Implemented with
  the native HTML5 drag-and-drop API (`web-sys`'s `DragEvent`/`DataTransfer`). Known
  limitation: this doesn't work on touch devices without extra work — there's no
  fallback control for mobile yet.
- **Hands render as one turn-ordered list, not separate "you" vs "others" sections.**
  The list starts from whoever's turn it is right now and wraps around in normal turn
  order, your own hand included at its natural position — so the top of the list is
  always who plays next, and a small "Now playing" badge reinforces it.
- **Hand cards are smaller** (2.3rem × 3.1rem, down from 3rem × 4rem) while staying
  rectangular, and the current-turn hand gets a brief highlight animation when it
  becomes someone's turn — driven by a CSS class toggle that re-triggers correctly
  even though the underlying hand blocks are now persistent DOM nodes (see below).
- **The turn-ordered hand list animates players swapping position** when the turn
  passes: each hand is a persistent DOM node (rendered via `<For>`, keyed by
  `PlayerId`, instead of being torn down and rebuilt every state update) so a
  hand-rolled FLIP effect can measure its old and new position and slide it there
  with a CSS transform transition, rather than the list just popping into its new
  order. No animation library — just `get_bounding_client_rect` + a forced reflow
  + a `transition`, in `game_board.rs`.
- **Fireworks show a progressively-revealed burst icon per color**, in the same
  spirit as the physical game's cards — where laying a suit's cards out in order
  reveals more of a small illustration. This is an original SVG design (not a
  reproduction of the physical cards' artwork), built from ray endpoints computed
  once and hard-coded rather than done with runtime trigonometry.
- **Discard pile groups cards by color** with small round chips instead of full-size
  card tiles, more legible at a glance and friendlier to small screens.
- **Each hand shows that player's last move** (clue given and to whom, card played
  and whether it succeeded, or card discarded) — useful for reading advanced plays
  like finesses, where correctly interpreting a clue depends on knowing exactly
  what happened on recent turns. Tracked server-side per player (`GameState::last_moves`)
  and included in every `PlayerView`.
- **Clueing is click-to-select, then pick from only the valid clues.** Click a
  player's name to select them as the clue target; the panel then shows buttons for
  only the colors/numbers actually present in their hand (computed client-side, since
  their cards are already visible to you) — so there's no way to attempt a clue the
  engine would reject for touching zero cards.
- **Frontend design**: dark night-sky background: the game's own five colors are the
  only bright accents (they're the actual fireworks, not arbitrary brand colors), cards
  render as color-filled tiles rather than generic rounded chips, clue/fuse tokens as
  pip rows (●○) rather than "6/8" text.
- **Variant rules are opt-in, chosen in the lobby before the game starts.** Any
  seated player can toggle them (`GameRules`, synced to everyone via `SetRules` /
  `RulesUpdated`); the server freezes whatever's selected into the `GameState` at
  `StartGame` and it can't change mid-game. See the rule lists below.

## Standard rules implemented

- 50-card deck: 5 colors × (three 1s, two 2s, two 3s, two 4s, one 5)
- Hand size: 5 cards for 2-3 players, 4 cards for 4-5 players
- 8 clue tokens, 3 fuse tokens
- A clue must touch at least one card in the target's hand
- Can't discard while at 8 clue tokens
- Completing a firework (playing a 5) refunds a clue token
- Game ends on: 3 fuses lost, all 5 fireworks completed, or one full round after the deck empties

## Optional rules

Toggled independently in the lobby, any combination:

- **Multicolor** (`multicolor`): a 6th suit, wild for color clues (a "Red" clue
  also touches multicolor cards) but can never be clued directly.
- **Black powder** (`black`): a suit with no color at all — no color clue,
  including naming it directly, ever touches it, the opposite of multicolor's
  "wild for every clue" — and its firework is built in *descending* order, 5
  down to 1, with a mirrored 1/2/2/2/3 card distribution (three 5s down to one
  1) to match.
- **Extra colors** (`extra_colors`, 0-2): a tri-state, not "N more suits on top of
  the base five" — White itself moves. 0: the plain five (white/red/yellow/green/blue).
  1: Orange *and* Purple both come in and White drops out to make room (6 suits:
  red/orange/yellow/green/blue/purple). 2: White comes back too, all seven at
  once. Orange and purple are perfectly ordinary suits — ascending, normal clue
  matching. One lobby control (a count, not per-color toggles).
- **Six-card suits** (`six_cards`): adds a 6th rank to every active suit — an
  ascending suit's unique 5 becomes a pair and 6 becomes the unique top card
  (3/2/2/2/2/1, 12 cards); black powder mirrors it (three 6s down to one 1).

Multicolor and black each add 5 to the max score; each extra color does too.
Each also adds 10 cards to the deck unless its "short" option below is on (all
combined: 90 cards, max score 45).

Each also has its own independent **"short deck"** option (`multicolor_short`,
`black_short`, `extra_colors_short` — the last applies uniformly to however
many extra colors are added): one copy of every rank (5 cards) instead of the
usual distribution, making those cards irreplaceable. Only matters if the
corresponding suit(s) are actually active.

## Hanabii mode

A game mode of its own (`hanabii`, spelled with two i's), picked in the lobby
under "Game mode". It's a fixed preset that **replaces** every option above rather
than combining with them: while it's on, the other lobby controls are locked (and
show what the mode plays with), and the server forces the same thing regardless of
what a client sends (`GameRules::normalized`, applied on `SetRules` and again when
the game is created).

- **Deck:** six colors — red, orange, yellow, green, blue, purple — with six cards
  each, 3/2/2/2/2/1 of ranks 1-6 (72 cards, max score 36). Exactly the deck of
  `extra_colors: 1` + `six_cards: true`.
- **Color clues:** only the three primary colors (red, yellow, blue) can be named
  (`ActionError::CannotClueSecondaryColor` otherwise). The other colors are mixed
  from them — **orange = red + yellow, green = yellow + blue, purple = red + blue**
  — and a clue touches every card whose color contains the named primary. So a red
  clue touches red, orange *and* purple cards; yellow touches yellow, orange and
  green; blue touches blue, green and purple. Number clues are unchanged.
- **A primary is always cluable, even touching nothing:** ordinarily a clue that
  touches no card is rejected, but in this mode all three primaries can always be
  given (`GameRules::allows_empty_color_clues`) — "none of your cards contain red" is
  exactly as informative as a hit, still costs a clue token and a turn, and every card
  in the hand gets the corresponding miss recorded. The lobby's clue buttons reflect
  this: all three primaries are always offered, whatever the target's hand holds.
- **What a player knows about their own cards:** a red hit means "red, orange or
  purple", not "red", so hanabii mode keeps its evidence as primary-color results
  (`CardKnowledge::hit_primaries` / `missed_primaries`) instead of a claimed color.
  While a card's color is uncertain it keeps a neutral face and gets a **spinning ring
  made of every color it could still be** (`CardKnowledge::hanabii_possible_colors`;
  equal hard-edged arcs, drawn from the `--ring-stops` style the app sets on the card —
  see `hanabii_ring_colors` in `game_board.rs` and `.card-ring` in `style.css`). Clues
  that *miss* a card narrow it down just as much as ones that touch it (a red miss
  leaves yellow, green and blue), so every ring spins. Once the clues leave a single
  possibility (red and yellow both hit → orange; red and yellow both missed → blue) the
  whole card fills in with that color and the ring goes away. There are no struck-through
  color marks in this mode: the ring already says everything.
- **The ring shows on every hand, not just your own:** other players' cards still show
  their true color as always, but now also carry the same ring their *owner* sees — so
  before clueing, you can tell at a glance what that player still doesn't know, and
  whether a clue would actually teach them anything.
- **Clue buttons are painted to match:** each primary's button blends into its two
  color-wheel neighbors at the edges — the colors that clue also touches — so the red
  button reads mostly red with a sliver of purple and orange at the sides
  (`clue_button_style` in `game_board.rs`).
- **Page title:** while the mode is on (ticked in the lobby, or being played) the page
  title — and the browser tab — read "Hanabii", and hovering (or focusing/tapping) the
  title opens a box with the mode's rules. The game screen itself carries no hanabii
  description paragraph.
- **Where the rules live:** `GameRules::color_clue_touches` / `clue_touches` are the
  single definition of what a clue touches, shared by the engine and the frontend's
  hover preview; `GameRules::cluable_colors` is what the clue buttons offer.

## Every mode: the last card gets played

The deck running out no longer ends the game outright. From the turn that draws the
final card, every player — that drawer included — gets exactly one more turn
(`GameState::final_turns_remaining`, counted down in `apply_action`), so the standard
rule holds: whoever draws the last card still gets to do something with it before the
game ends. `EndReason::DeckExhausted` fires once the countdown reaches zero, and losing
all fuses or completing every firework still ends the game immediately even mid-countdown.

## Every mode: what the latest clue touched

`LastMove::Clue` now carries exactly which card ids a clue touched (`touched:
Vec<CardId>`, replacing the old `touched_count`), and `GameState`/`PlayerView` record
who took the most recent turn (`last_actor`). The frontend uses this to briefly
highlight the touched cards — a white pulse, `.card-touched` — on *every* screen for a
couple of seconds after the clue: the target's own hand and whoever gave it.

## Server protocol (v1)

A client connects to `/ws`, and the *first* message must be a `Join`. After that,
before the game starts, a seated player can send `SetRules` to change the room's
selected variant rules (echoed to everyone as `RulesUpdated`) or `StartGame` (any
seated player, once 2+ have joined) to lock in the current rules and begin.
Once running, `Action` carries normal game moves. See `game-core/src/messages.rs`
for the exact `ClientMessage` / `ServerMessage` shapes.
