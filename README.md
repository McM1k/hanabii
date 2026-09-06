# Hanabi (working title)

A multiplayer clone of the cooperative card game Hanabi, built in Rust end-to-end:
Axum for the server, Leptos (WASM) for the frontend, WebSockets tying them together.

## Status

- [x] `game-core` — the rules engine. Compiler-verified, 18/18 tests passing.
- [x] `server` — Axum + WebSockets, room management. Compiler-verified, runs, join
      flow tested manually.
- [x] `frontend` — Leptos UI. Written, **not yet compiler-verified**. This is the
      riskiest crate in the project — Leptos's reactive/view-macro API and the
      `gloo-net` WebSocket client are areas I have real uncertainty about, more
      than anything in `game-core` or `server`. Expect this one to take a couple
      of rounds of fixes.

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
  becomes someone's turn — CSS-only, no new Rust logic, and works naturally because
  each hand block is a freshly-rendered DOM node every turn rather than a persistent
  one being toggled.
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
- **Standard rules only, for now.** Tweaks are still TBD.

## Standard rules implemented

- 50-card deck: 5 colors × (three 1s, two 2s, two 3s, two 4s, one 5)
- Hand size: 5 cards for 2-3 players, 4 cards for 4-5 players
- 8 clue tokens, 3 fuse tokens
- A clue must touch at least one card in the target's hand
- Can't discard while at 8 clue tokens
- Completing a firework (playing a 5) refunds a clue token
- Game ends on: 3 fuses lost, all 5 fireworks completed, or one full round after the deck empties

## Server protocol (v1)

A client connects to `/ws`, and the *first* message must be a `Join`. Everything
after that is either `StartGame` (any seated player, once 2+ have joined) or
`Action` (a normal game move). See `game-core/src/messages.rs` for the exact
`ClientMessage` / `ServerMessage` shapes.
