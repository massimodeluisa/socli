//! What the composer publishes: the text, the networks, the post it answers and the draft it came from.

use crate::net::Network;

/// One publication exactly as the composer shows it when it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    /// The text to send, converted unless the composer is raw.
    pub text: String,
    /// The text as typed, which a draft keeps so a later edit shows what the user wrote.
    pub typed: String,
    pub targets: Vec<Network>,
    pub reply_to: Option<String>,
    pub quote: Option<String>,
    pub draft_id: Option<String>,
}

impl Intent {
    /// The checks an intent passes again before it is sent or dry-run.
    pub fn check(&self) -> Result<(), String> {
        if self.targets.is_empty() {
            return Err("no network to post to".into());
        }
        Ok(())
    }

    /// The slugs of `targets`.
    pub fn slugs(&self) -> Vec<String> {
        self.targets
            .iter()
            .map(|net| net.slug().to_string())
            .collect()
    }
}
