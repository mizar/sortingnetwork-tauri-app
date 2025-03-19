use leptos::prelude::*;
//use crate::components::greet::Greet;
//use crate::components::event_frontend::EventFrontend;
//use crate::components::event_backend::EventBackend;
use crate::components::sorting_network_verify::SortingNetworkVerify;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="container">
            <SortingNetworkVerify />
        </main>
    }
}
