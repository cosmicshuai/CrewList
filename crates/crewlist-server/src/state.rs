//! Shared handler state.

use crate::repo::SharedRepo;

#[derive(Clone)]
pub struct AppState {
    pub repo: SharedRepo,
}

impl AppState {
    pub fn new(repo: SharedRepo) -> Self {
        Self { repo }
    }
}
