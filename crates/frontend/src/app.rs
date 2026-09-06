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

#[component]
pub fn App() -> impl IntoView {
    let ctx = AppContext::new();
    provide_context(ctx);

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
            <h1>"Hanabi"</h1>
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
