//! Shared handler state.

use crewlist_store::Stores;

#[derive(Clone)]
pub struct AppState {
    pub stores: Stores,
}

impl AppState {
    pub fn new(stores: Stores) -> Self {
        Self { stores }
    }
}
