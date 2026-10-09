//! FR-QA-012: the language the answer is written in, told to the model by name. Short messages
//! ("selam", "130 kiloyum", "ok") say little: the earlier question's language, then PLA's own
//! language decide them (nl-quality plan § 2; "selam" was answered in English in the real window).

/// Words that only Turkish writes this way (also without Turkish letters, as people type them).
const TR_WORDS: &[&str] = &[
    "ve", "bir", "bu", "su", "o", "ne", "mi", "mı", "mu", "mü", "için", "icin", "nasıl", "nasil", "kaç", "kac", "hangi", "yarın", "yarin", "bugün", "bugun", "dün",
    "dun", "gün", "gun", "var", "yok", "ben", "benim", "sen", "saat", "lazım", "lazim", "gerek", "ile", "neler", "selam", "slm", "merhaba", "naber", "sa",
    "tamam", "evet", "hayır", "hayir", "teşekkür", "tesekkur", "teşekkürler", "tesekkurler", "sağol", "sagol", "iyi", "günaydın", "gunaydin", "kilo", "ekle",
    "bunu", "onu", "şunu", "ama", "çok", "cok", "da", "de", "ki", "gibi", "daha", "olarak", "kere", "adım", "adim", "bardak", "litre", "hafta", "ay", "yıl",
];

/// Turkish endings, for words like "kiloyum", "uyudum", "gidecegim" (four letters or more).
const TR_ENDINGS: &[&str] = &[
    "yum", "yim", "yüm", "yım", "dum", "dim", "düm", "dım", "tum", "tim", "tüm", "tım", "iyor", "ıyor", "uyor", "üyor", "acak", "ecek", "acağım", "eceğim",
    "ecegim", "acagim", "mış", "miş", "muş", "müş", "lerim", "larım", "ları", "leri", "sın", "sin", "mısın", "misin", "dık", "dik", "duk", "dük", "lım", "lim",
];

const EN_WORDS: &[&str] = &[
    "the", "a", "an", "and", "i", "i'm", "you", "my", "me", "is", "are", "was", "were", "to", "of", "in", "on", "at", "it", "this", "that", "what", "how", "when",
    "did", "do", "does", "have", "has", "will", "can", "please", "hi", "hello", "hey", "thanks", "thank", "yes", "today", "tomorrow", "yesterday", "weigh",
    "slept", "walked", "drank", "remind", "note", "save", "need", "last", "night", "week", "hours", "steps", "much", "many", "with", "for", "about",
];

fn score(text: &str) -> (usize, usize) {
    let lower = text.to_lowercase();
    let mut tr = 3 * usize::from(lower.chars().any(|c| "çğıöşü".contains(c)));
    let mut en = 0;
    for w in lower.split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()) {
        let w = w.split('\'').next().unwrap_or(w); // "10'da" → "10", "i'm" stays a word below
        if TR_WORDS.contains(&w) || (w.chars().count() >= 4 && TR_ENDINGS.iter().any(|e| w.ends_with(e))) {
            tr += 1;
        }
        if EN_WORDS.contains(&w) {
            en += 1;
        }
    }
    (tr, en)
}

/// `tr` or `en` for `text`, or `None` when it does not tell.
fn guess(text: &str) -> Option<&'static str> {
    match score(text) {
        (tr, en) if tr > en => Some("tr"),
        (tr, en) if en > tr => Some("en"),
        _ => None,
    }
}

/// The answer's language: the question's own, else the earlier question's, else PLA's (`ui`).
pub fn answer_language(question: &str, earlier: Option<&str>, ui: &str) -> &'static str {
    guess(question).or_else(|| earlier.and_then(guess)).unwrap_or(if ui == "en" { "en" } else { "tr" })
}

/// The name the model is told.
pub fn name(code: &str) -> &'static str {
    if code == "en" { "English" } else { "Turkish" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish_and_english_questions() {
        assert_eq!(answer_language("Bugün ne yaptım?", None, "en"), "tr");
        assert_eq!(answer_language("Yarin doktora gidecegim", None, "en"), "tr", "without Turkish letters");
        assert_eq!(answer_language("How many hours did I sleep?", None, "tr"), "en");
        assert_eq!(answer_language("I have to send the report to Ali tomorrow", None, "tr"), "en");
    }

    #[test]
    fn short_messages_seen_in_the_real_window() {
        // the owner's screenshots: both were answered in English
        assert_eq!(answer_language("selam", None, "en"), "tr");
        assert_eq!(answer_language("130 kiloyum", None, "en"), "tr");
        assert_eq!(answer_language("notlarıma ekle bunu", None, "en"), "tr");
        assert_eq!(answer_language("sen karar ver", None, "en"), "tr");
        assert_eq!(answer_language("hi", None, "tr"), "en");
    }

    #[test]
    fn a_message_that_does_not_tell_follows_the_conversation_then_pla() {
        assert_eq!(answer_language("ok", Some("what is on my list tomorrow?"), "tr"), "en");
        assert_eq!(answer_language("ok", Some("yarın neler var"), "en"), "tr");
        assert_eq!(answer_language("ok", None, "en"), "en");
        assert_eq!(answer_language("👍", None, "tr"), "tr");
    }
}
