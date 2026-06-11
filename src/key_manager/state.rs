use std::time::Instant;

#[derive(Clone)]
pub struct KeyState {
    pub name: String,
    pub key: String,

    pub cooldown_until: Option<Instant>,

    pub successes: u64,
    pub failures: u64,
}
