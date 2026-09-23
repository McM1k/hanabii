use leptos::*;

use crate::game_board::GameBoard;
use crate::screens::{JoinScreen, Lobby};
use crate::ws::AppContext;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Joining,
    Lobby,
    Playing,
}

/// The rules of the hanabii mode as shown in the box that appears when the
/// page title is hovered: the clue rule first, then how your own cards show
/// what you know.
const HANABII_CLUE_RULES: &str = "Only red, yellow and blue can be clued — and always, even when a clue touches nothing, since ruling a color out is information too. Orange is red + yellow, green is yellow + blue and purple is red + blue, so a red clue touches every red, orange and purple card, and so on.";
const HANABII_CARD_MARKERS: &str = "A spinning ring shows every color a card could still be — on your own cards, and on everyone else's so you can see what they know — and the whole card fills in once its color is certain.";

#[component]
pub fn App() -> impl IntoView {
    let ctx = AppContext::new();
    provide_context(ctx);

    // Whether the hanabii mode is on: what the lobby has picked until the
    // game starts, then what the game is actually being played with. A memo,
    // so nothing below re-renders on the (many) state updates that don't
    // change the answer.
    let hanabii = create_memo(move |_| {
        ctx.view.with(|view| match view {
            Some(view) => view.rules.hanabii,
            None => ctx.rules.with(|rules| rules.hanabii),
        })
    });

    // The browser tab follows the on-page title.
    create_effect(move |_| {
        document().set_title(if hanabii.get() { "Hanabii" } else { "Hanabi" });
    });

    let stage = move || {
        if ctx.my_id.get().is_none() {
            Stage::Joining
        } else if !ctx.game_started.get() {
            Stage::Lobby
        } else {
            Stage::Playing
        }
    };

    view! {
        <main class="app">
            {move || {
                if hanabii.get() {
                    // Focusable, so the box also opens on a tap or from the
                    // keyboard where there's no hover.
                    view! {
                        <div class="page-title page-title-hanabii" tabindex="0">
                            <h1>"Hanabii"</h1>
                            <div class="mode-popover" role="tooltip">
                                <p>{HANABII_CLUE_RULES}</p>
                                <p>{HANABII_CARD_MARKERS}</p>
                            </div>
                        </div>
                    }
                    .into_view()
                } else {
                    view! {
                        <div class="page-title">
                            <h1>"Hanabi"</h1>
                        </div>
                    }
                    .into_view()
                }
            }}
            {move || {
                let msg = ctx.status.get();
                (!msg.is_empty()).then(|| view! { <p class="status">{msg}</p> })
            }}
            {move || match stage() {
                Stage::Joining => view! { <JoinScreen/> }.into_view(),
                Stage::Lobby => view! { <Lobby/> }.into_view(),
                Stage::Playing => view! { <GameBoard/> }.into_view(),
            }}
        </main>
    }
}
