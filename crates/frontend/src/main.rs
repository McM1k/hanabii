mod app;
mod game_board;
mod screens;
mod ws;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(|| leptos::view! { <App/> });
}
