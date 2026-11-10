//! Reconciles the two shapes opencode streams a message part in (`lib/partAccumulator.ts`). For one text part the server sends both
//! `message.part.delta` (the text just appended) and `message.part.updated` (the whole text so far): the same bytes twice. This keeps
//! how much of each part was handed out and returns only the unseen suffix, whichever event delivers it.

use std::collections::HashMap;

#[derive(Default)]
pub struct Accumulator {
    emitted: HashMap<String, usize>,
    total: usize,
}

impl Accumulator {
    /// An increment: all of it is new.
    pub fn delta(&mut self, part_id: &str, increment: &str) -> Option<String> {
        if increment.is_empty() {
            return None;
        }
        let length = increment.chars().count();
        *self.emitted.entry(part_id.to_string()).or_insert(0) += length;
        self.total += length;
        Some(increment.to_string())
    }

    /// The whole text so far. Shorter than what was handed out means the server rewrote the part rather than extending it:
    /// re-sending it would duplicate, and a later snapshot will carry whatever is missing.
    pub fn snapshot(&mut self, part_id: &str, full: &str) -> Option<String> {
        let seen = self.emitted.get(part_id).copied().unwrap_or(0);
        let length = full.chars().count();
        if length <= seen {
            return None;
        }
        self.emitted.insert(part_id.to_string(), length);
        self.total += length - seen;
        Some(full.chars().skip(seen).collect())
    }

    /// Characters handed out across every part: "did this turn say anything".
    pub fn emitted_chars(&self) -> usize {
        self.total
    }
}
