//! Word-boundary matching with JavaScript's ASCII word characters.

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\bneedle\b` with JavaScript's ASCII word characters.
pub fn has_word(hay: &str, needle: &str) -> bool {
    let bytes = hay.as_bytes();
    hay.match_indices(needle).any(|(i, _)| {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + needle.len()).copied();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}
