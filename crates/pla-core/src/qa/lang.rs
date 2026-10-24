//! FR-QA-012: the language the answer is written in, told to the model by name. Short messages
//! ("selam", "130 kiloyum", "ok") say little: the earlier question's language, then PLA's own
//! language decide them (nl-quality plan § 2; "selam" was answered in English in the real window).

/// Words that only Turkish writes this way (also without Turkish letters, as people type them).
const TR_WORDS: &[&str] = &[
    "ve",
    "bir",
    "bu",
    "su",
    "o",
    "ne",
    "mi",
    "mı",
    "mu",
    "mü",
    "için",
    "icin",
    "nasıl",
    "nasil",
    "kaç",
    "kac",
    "hangi",
    "yarın",
    "yarin",
    "bugün",
    "bugun",
    "dün",
    "dun",
    "gün",
    "gun",
    "var",
    "yok",
    "ben",
    "benim",
    "sen",
    "saat",
    "lazım",
    "lazim",
    "gerek",
    "ile",
    "neler",
    "selam",
    "slm",
    "merhaba",
    "naber",
    "sa",
    "tamam",
    "evet",
    "hayır",
    "hayir",
    "teşekkür",
    "tesekkur",
    "teşekkürler",
    "tesekkurler",
    "sağol",
    "sagol",
    "iyi",
    "günaydın",
    "gunaydin",
    "kilo",
    "ekle",
    "bunu",
    "onu",
    "şunu",
    "ama",
    "çok",
    "cok",
    "da",
    "de",
    "ki",
    "gibi",
    "daha",
    "olarak",
    "kere",
    "adım",
    "adim",
    "bardak",
    "litre",
    "hafta",
    "ay",
    "yıl",
];

/// Turkish endings, for words like "kiloyum", "uyudum", "gidecegim" (four letters or more).
const TR_ENDINGS: &[&str] = &[
    "yum", "yim", "yüm", "yım", "dum", "dim", "düm", "dım", "tum", "tim", "tüm", "tım", "iyor", "ıyor", "uyor", "üyor", "acak", "ecek", "acağım", "eceğim",
    "ecegim", "acagim", "mış", "miş", "muş", "müş", "lerim", "larım", "ları", "leri", "sın", "sin", "mısın", "misin", "dık", "dik", "duk", "dük", "lım", "lim",
];

const EN_WORDS: &[&str] = &[
    // not "a", "on", "at", "can": Turkish too ("mail at", "Can'a yaz", final review M2)
    "the",
    "an",
    "and",
    "i",
    "i'm",
    "you",
    "my",
    "me",
    "is",
    "are",
    "was",
    "were",
    "to",
    "of",
    "in",
    "it",
    "this",
    "that",
    "what",
    "how",
    "when",
    "did",
    "do",
    "does",
    "have",
    "has",
    "will",
    "please",
    "hi",
    "hello",
    "hey",
    "thanks",
    "thank",
    "yes",
    "today",
    "tomorrow",
    "yesterday",
    "weigh",
    "slept",
    "walked",
    "drank",
    "remind",
    "note",
    "save",
    "need",
    "last",
    "night",
    "week",
    "hours",
    "steps",
    "much",
    "many",
    "with",
    "for",
    "about",
];

fn score(text: &str) -> (usize, usize) {
    let lower = text.to_lowercase();
    let mut tr = 3 * usize::from(lower.chars().any(|c| "çğıöşü".contains(c)));
    let mut en = 0;
    for w in lower.split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()) {
        // a case ending after an apostrophe is Turkish ("Ali'ye", "Can'a", "10'da"); "'s", "'m" English
        if let Some((_, ending)) = w.split_once('\'') {
            if [
                "a", "e", "ya", "ye", "da", "de", "ta", "te", "dan", "den", "tan", "ten", "ın", "in", "un", "ün", "nın", "nin", "la", "le", "yla", "yle", "ı",
                "i", "yı", "yi",
            ]
            .contains(&ending)
            {
                tr += 1;
            } else if ["s", "m", "re", "ll", "ve", "t", "d"].contains(&ending) {
                en += 1;
            }
        }
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

/// `tr` or `en` for `text`, or `None` when it does not tell (the memory guard's language check).
pub fn language_of(text: &str) -> Option<&'static str> {
    guess(text)
}

/// The answer's language: the question's own, else the earlier question's, else PLA's (`ui`).
pub fn answer_language(question: &str, earlier: Option<&str>, ui: &str) -> &'static str {
    guess(question).or_else(|| earlier.and_then(guess)).unwrap_or(if ui == "en" { "en" } else { "tr" })
}

/// Words that make a sentence a guess, a denial, a wish or a proposal, not a fact (memory guard,
/// spec 2026-10-18). Folded as `index::loose_words` reads them: no Turkish letters ("değil" is
/// "degil").
const UNSURE_WORDS: &[&str] = &[
    "belki", "sanirim", "galiba", "herhalde", "muhtemelen", "acaba", "olabilir", "keske", "eger", "degil", "dusunuyorum", "planliyorum", "maybe", "might",
    "perhaps", "probably", "guess", "think", "not", "never", "if", "unless", "wish", "let",
];

/// The question particle, alone or with its person ending ("mı", "misin", "muyuz").
const QUESTION_WORDS: &[&str] = &[
    "mi", "mu", "misin", "musun", "miyim", "muyum", "miyiz", "muyuz", "misiniz", "musunuz", "midir", "mudur", "miydi", "muydu", "miymis", "muymus",
];

/// Conditional and wish endings ("yaparsa", "gelirse", "yapsam"), on words of five letters or
/// more so "masa" or "sen" stay out; and the "let us" ending ("yapalım", "gidelim") on six or more.
const CONDITIONAL_ENDINGS: &[&str] =
    &["rsa", "rse", "zsa", "zse", "ysa", "yse", "rsam", "rsem", "rsan", "rsen", "rsak", "rsek", "sam", "sem", "saydi", "seydi", "saniz", "seniz"];
const PROPOSAL_ENDINGS: &[&str] = &["alim", "elim"];
/// Words that only look conditional ("akşam" ends like "yapsam").
const NOT_CONDITIONAL: &[&str] = &["aksam"];

/// Words that tie a clause to one day or week ("bugün yorgunum"): true now, not lasting.
const PASSING_WORDS: &[&str] = &["bugun", "dun", "yarin", "simdilik", "today", "tomorrow", "yesterday", "tonight", "will", "gonna", "going"];
const PASSING_PAIRS: &[[&str; 2]] = &[["bu", "hafta"], ["bu", "sabah"], ["bu", "aksam"], ["bu", "gece"], ["this", "week"], ["this", "morning"]];

/// Future endings ("gideceğim", "yapacak"): a plan, not a fact yet.
const FUTURE_ENDINGS: &[&str] = &["acagim", "ecegim", "acagiz", "ecegiz", "acaksin", "eceksin", "acak", "ecek"];

/// Whether a sentence hedges, denies, asks, sets a condition or proposes: such a sentence is never
/// written to memory as a fact.
pub fn is_unsure(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    if lower.contains('?') || lower.contains("n't") || lower.contains("n’t") {
        return true;
    }
    crate::index::loose_words(sentence).iter().any(|w| {
        let n = w.chars().count();
        UNSURE_WORDS.contains(&w.as_str())
            || QUESTION_WORDS.contains(&w.as_str())
            || (n >= 5 && !NOT_CONDITIONAL.contains(&w.as_str()) && CONDITIONAL_ENDINGS.iter().any(|e| w.ends_with(e)))
            || (n >= 6 && PROPOSAL_ENDINGS.iter().any(|e| w.ends_with(e)))
    })
}

/// Whether a clause speaks of one day or of a plan ("yarın gideceğim", "bu hafta gidemedim"): true
/// now or later, not lasting. Read per clause, so "bugün uğradım, kahveyi sütsüz içiyorum" keeps
/// its second half.
pub fn is_passing(clause: &str) -> bool {
    let words = crate::index::loose_words(clause);
    words.windows(2).any(|p| PASSING_PAIRS.iter().any(|q| p[0] == q[0] && p[1] == q[1]))
        || words.iter().any(|w| PASSING_WORDS.contains(&w.as_str()) || FUTURE_ENDINGS.iter().any(|e| w.ends_with(e) && w.len() > e.len() + 1))
}

/// Passive general-present endings, folded ("sulanır", "düzenlenir", "çekilir"): how a thing is
/// done or is, not where the user's work stands.
const GENERAL_ENDINGS: &[&str] = &["lir", "lur", "nir", "nur"];

/// Whether a sentence states how something is in general ("Monstera haftada bir sulanır.") rather
/// than a state: its last word is a passive general present. Short words ("gelir", "bilir") are
/// left alone.
pub fn is_general(sentence: &str) -> bool {
    crate::index::loose_words(sentence).last().is_some_and(|w| w.chars().count() >= 6 && GENERAL_ENDINGS.iter().any(|e| w.ends_with(e)))
}

/// Whether a sentence passes on what the user heard ("taşınacakmış", "artırmış"): some clause
/// ends in the reported past. Someone else's news is not the user's state.
pub fn is_hearsay(sentence: &str) -> bool {
    sentence.split([',', ';', '.', '!', '\n']).any(|clause| {
        crate::index::loose_words(clause)
            .last()
            .is_some_and(|w| w.chars().count() >= 5 && !NOT_HEARSAY.contains(&w.as_str()) && HEARSAY_ENDINGS.iter().any(|e| w.ends_with(e)))
    })
}

/// The reported past, folded ("-mış", "-mişler").
const HEARSAY_ENDINGS: &[&str] = &["mis", "mus", "misler", "mislar", "muslar", "musler"];
/// Words with that ending that are not a verb.
const NOT_HEARSAY: &[&str] = &["gecmis", "yetmis", "doksanmis"];

/// Endings a list item has once it is a clause ("tutuldu", "geliyor", "installed"), folded.
const VERB_ENDINGS: &[&str] = &["di", "du", "ti", "tu", "yor", "yoruz", "dir", "tir", "mis", "mus", "cak", "cek", "ed"];

/// Whether a sentence is a bare list ("Süt, yumurta, ekmek, domates."): three or more short items
/// and no verb among them.
pub fn is_list(sentence: &str) -> bool {
    let items: Vec<Vec<String>> = sentence.trim().trim_end_matches(['.', '!']).split(',').map(crate::index::loose_words).collect();
    let verb = |w: &String| (w.chars().count() >= 5 && VERB_ENDINGS.iter().any(|e| w.ends_with(e))) || ["is", "are", "was", "were"].contains(&w.as_str());
    items.len() >= 3 && items.iter().all(|i| !i.is_empty() && i.len() <= 3 && !i.iter().any(verb))
}

/// Parts of a day, folded: a sentence that names one tells what happened then.
const DAY_PARTS: &[&str] = &["sabah", "sabahleyin", "ogle", "ogleden", "oglen", "aksam", "aksamleyin", "gece", "geceleyin", "morning", "afternoon", "evening", "tonight"];

/// Whether a sentence tells what happened at one part of a day ("Akşam yemeğe çıktık", "in the
/// morning"): a diary line, not where the user's work stands.
pub fn is_day_event(sentence: &str) -> bool {
    crate::index::loose_words(sentence).iter().any(|w| DAY_PARTS.contains(&w.as_str()))
}

/// The name the model is told.
pub fn name(code: &str) -> &'static str {
    if code == "en" {
        "English"
    } else {
        "Turkish"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_statements_are_not_states() {
        for s in ["Monstera haftada bir sulanır.", "git stash ile değişiklik saklanır", "DÜŞEYARA ile değer çekilir."] {
            assert!(is_general(s), "{s}");
        }
        for s in ["Tezin yöntem bölümünü yazıyorum.", "Ayda bir gelir.", "Motor siparişi verildi."] {
            assert!(!is_general(s), "{s}");
        }
    }

    #[test]
    fn heard_news_lists_and_day_events_are_not_states() {
        for s in ["Alt kattaki aile yaz sonunda taşınacakmış.", "Yönetici aidatı artırmış", "Komşular gitmişler."] {
            assert!(is_hearsay(s), "{s}");
        }
        for s in ["Geçmiş projeler arşivde.", "Motor siparişi verildi.", "Raporu teslim ettim."] {
            assert!(!is_hearsay(s), "{s}");
        }
        for s in ["Passport, charger, two shirts, sunscreen.", "Süt, yumurta, ekmek, domates."] {
            assert!(is_list(s), "{s}");
        }
        for s in ["Salon tutuldu, davetiyeler basıldı, pasta seçildi.", "X8 seçildi, motor kargoda.", "Cabinets arrived, installed, painted and checked."] {
            assert!(!is_list(s), "{s}");
        }
        for s in ["Öğleden sonra kütüphanede çalıştım.", "Akşam arkadaşlarla yemeğe çıktık.", "Went for a run in the morning."] {
            assert!(is_day_event(s), "{s}");
        }
        for s in ["Bugün bitirme projesinin raporunu teslim ettim.", "Tez taslağını danışmanıma gönderdim."] {
            assert!(!is_day_event(s), "{s}");
        }
    }

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
        assert_eq!(answer_language("Ali'ye mail at", None, "en"), "tr", "final review M2");
        assert_eq!(answer_language("Can'a mesaj at yarın", None, "en"), "tr");
    }

    #[test]
    fn a_message_that_does_not_tell_follows_the_conversation_then_pla() {
        assert_eq!(answer_language("ok", Some("what is on my list tomorrow?"), "tr"), "en");
        assert_eq!(answer_language("ok", Some("yarın neler var"), "en"), "tr");
        assert_eq!(answer_language("ok", None, "en"), "en");
        assert_eq!(answer_language("👍", None, "tr"), "tr");
    }
}
