//! Small helpers every layer uses: change detection, a token estimate, record ids. Here, not in
//! `pipeline` or `qa`, so files, the index and memory need not reach up into them (architecture
//! review A1).

/// FNV-1a 64-bit — only answers "did this change?" (FR-EXT-007, FR-VLT-012), not a security hash.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A cautious token count: about 3 characters a token for words, one token for each digit (the
/// tokenizer splits numbers, and tool results are full of dates and ids).
pub fn tokens(s: &str) -> usize {
    let digits = s.chars().filter(char::is_ascii_digit).count();
    (s.chars().count() - digits).div_ceil(3) + digits
}

/// A new record id: a random UUID as 32 hex digits.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_fnv1a_64_in_hex() {
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"a"), "af63dc4c8601ec8c");
        assert_ne!(content_hash("yarın".as_bytes()), content_hash("yarin".as_bytes()));
    }

    #[test]
    fn tokens_count_words_by_three_letters_and_digits_one_by_one() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("abcdef"), 2);
        assert_eq!(tokens("2026"), 4);
        assert_eq!(tokens("şğü"), 1, "letters, not bytes");
    }

    #[test]
    fn ids_are_32_hex_digits_and_fresh() {
        let (a, b) = (new_id(), new_id());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
