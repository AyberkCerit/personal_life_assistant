//! Checks on the model's items that need the note's own words (nl-quality plan § 2). The model
//! read a pasted workout program ("Bench press: 4x4-6") as four workouts done today at 6 kg.

use super::{ItemType, MetricKind, RawExtraction, Weekday};

/// "4x4-6", "3-4x5", "3x8–10": a range of sets or repetitions belongs to a program, not to a
/// workout someone did (they did one number). The dash sits right between the numbers: "3x10 - 60
/// kg" is a log with a weight (final review I4).
fn has_rep_range(text: &str) -> bool {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let digits_end = |mut i: usize| {
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let spaces_end = |mut i: usize| {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        i
    };
    let is_dash = |i: usize| i < chars.len() && (chars[i] == '-' || chars[i] == '–');
    let is_times = |i: usize| i < chars.len() && "x×*".contains(chars[i]);
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        let a = digits_end(i);
        // N x N-N
        let b = spaces_end(a);
        if is_times(b) {
            let c = spaces_end(b + 1);
            let d = digits_end(c);
            if d > c && is_dash(d) && digits_end(d + 1) > d + 1 {
                return true;
            }
        }
        // N-N x N
        if is_dash(a) && digits_end(a + 1) > a + 1 {
            let e = spaces_end(digits_end(a + 1));
            if is_times(e) && digits_end(spaces_end(e + 1)) > spaces_end(e + 1) {
                return true;
            }
        }
        i = a.max(i + 1);
    }
    false
}

/// Turkish letters folded too: people type "deneyecegim" as often as "deneyeceğim".
fn words(text: &str) -> Vec<String> {
    crate::index::loose_words(text)
}

/// A plan or program, word for word: not "plank", "planlama", "programming" or "Bulgarian split
/// squat" (final review I4).
fn names_a_plan(text: &str) -> bool {
    const PLAN: [&str; 14] =
        ["program", "programi", "programim", "programimiz", "programs", "plan", "plani", "planim", "planimiz", "plans", "routine", "rutin", "rutinim", "split"];
    let w = words(text);
    w.iter().enumerate().any(|(i, x)| PLAN.contains(&x.as_str()) && !(x == "split" && w.get(i + 1).is_some_and(|n| n == "squat")))
}

/// A workout still to come: a future verb or "haftaya" in the block, and no past verb ("yaptım", "did")
/// that would make it a log written beside a plan for later.
fn is_future(text: &str) -> bool {
    let w = words(text);
    let past =
        w.iter().any(|x| x.len() > 4 && ["dim", "dum", "tim", "tum"].iter().any(|e| x.ends_with(e)) || ["did", "done", "finished"].contains(&x.as_str()));
    !past
        && w.iter().any(|w| {
            ["acagim", "ecegim", "acagiz", "ecegiz", "acak", "ecek"].iter().any(|e| w.ends_with(e) && w.len() > e.len() + 1) || w == "will" || w == "haftaya"
        })
}

/// Whether the note gives a weight unit at all; without one a workout's number is not kilograms.
fn names_a_weight(text: &str) -> bool {
    words(text).iter().any(|w| {
        (w.starts_with("kg")
            || (w.starts_with("kilo") && !w.starts_with("kilomet"))
            || w == "lb"
            || w == "lbs"
            || w.starts_with("pound")
            || w.starts_with("libre"))
            || (w.ends_with("kg") && w.trim_end_matches("kg").chars().all(|c| c.is_ascii_digit() || c == '.' || c == ','))
    })
}

const GOAL: [&str; 6] = ["hedef", "istiyor", "olmak", "goal", "target", "want"];

/// The numbers written in `text`, "82,5" and "82.5" as one.
fn numbers(text: &str) -> Vec<f64> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || ((chars[j] == ',' || chars[j] == '.') && chars.get(j + 1).is_some_and(char::is_ascii_digit)))
            {
                j += 1;
            }
            let n: String = chars[i..j].iter().map(|c| if *c == ',' { '.' } else { *c }).collect();
            out.extend(n.parse::<f64>().ok());
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The body weight `value` is the one wished for: it sits in the clause with the goal word ("75 kilo
/// olmak istiyorum", "hedefim 75 kiloya inmek"); "bugün 82 kiloyum, 75 olmak istiyorum" keeps 82
/// (final review I4).
fn is_goal_value(text: &str, value: Option<f64>) -> bool {
    let lower = text.to_lowercase();
    let mut clauses: Vec<&str> = vec![lower.as_str()];
    for sep in [", ", "; ", " ama ", " but ", " ve ", " and "] {
        clauses = clauses.iter().flat_map(|c| c.split(sep)).collect();
    }
    let goals: Vec<&&str> = clauses.iter().filter(|c| GOAL.iter().any(|g| c.contains(g))).collect();
    match value {
        None => !goals.is_empty(),
        Some(v) => goals.iter().any(|c| numbers(c).iter().any(|n| (n - v).abs() < 1e-9)),
    }
}

/// The part of `note` a block belongs to: the lines between blank lines around it, as a slice of
/// the note (byte positions kept while splitting, final review M5). A list item is a block of its
/// own, so "- Squat: 4x5" alone does not show that "- Deadlift: 3-4x5" next to it makes the whole
/// list a program (seen in the real window).
pub fn section_of<'a>(note: &'a str, block: &'a str) -> &'a str {
    let wanted = block.trim().replace('\r', "");
    if wanted.is_empty() {
        return block;
    }
    let mut start: Option<usize> = None;
    let mut offset = 0;
    let mut sections: Vec<(usize, usize)> = Vec::new();
    for line in note.split_inclusive('\n') {
        let blank = line.trim().is_empty();
        match (blank, start) {
            (false, None) => start = Some(offset),
            (true, Some(s)) => {
                sections.push((s, offset));
                start = None;
            }
            _ => {}
        }
        offset += line.len();
    }
    if let Some(s) = start {
        sections.push((s, note.len()));
    }
    sections.iter().map(|(s, e)| &note[*s..*e]).find(|s| s.replace('\r', "").contains(&wanted)).unwrap_or(block)
}

/// Whether `text` gives a clock time: a number, or a word that is one ("öğlen", "noon", "gece
/// yarısı", "midnight"). "Yatmadan" or "akşam" alone is no time; the model wrote 22:00 for it (the
/// owner's report). The assistant does the same with `without_made_up_time`.
pub fn states_a_time(text: &str) -> bool {
    let w = words(text);
    text.chars().any(|c| c.is_ascii_digit())
        || w.iter().any(|x| x.starts_with("oglen") || x == "noon" || x == "midnight")
        || w.windows(2).any(|p| p[0] == "gece" && p[1].starts_with("yarisi"))
}

const ALL_DAYS: [Weekday; 7] = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun];

/// A habit sentence that names no day ("Her akşam yatmadan kelime tekrarı"): every day. The model
/// copied the days of the habit before it in the same paragraph (the owner's report).
fn every_day(sentence: &str) -> bool {
    const DAYS: [&str; 17] = [
        "pazar", "sali", "salı", "carsamba", "çarsamba", "persembe", "cuma", "hafta", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
        "weekday", "weekend",
    ];
    let w = words(sentence);
    let names_a_day = w.iter().any(|x| DAYS.iter().any(|d| x.starts_with(d)));
    let every = w.iter().any(|x| x == "her" || x == "hergun" || x == "every" || x == "daily" || x == "each");
    every && !names_a_day
}

/// The sentences of `text`: split at ".", "!", "?" and line ends, but not inside "7.30".
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let decimal = c == '.' && k > 0 && chars[k - 1].1.is_ascii_digit() && chars.get(k + 1).is_some_and(|n| n.1.is_ascii_digit());
        if ".!?\n".contains(c) && !decimal {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&text[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// The sentence of `block` an item titled `title` comes from: the one sharing the most word stems
/// (five letters: "spora" ~ "Spor"); the whole block when none shares one.
fn sentence_of<'a>(block: &'a str, title: &str) -> &'a str {
    let stems = |s: &str| words(s).into_iter().map(|w| w.chars().take(5).collect::<String>()).collect::<std::collections::BTreeSet<_>>();
    let wanted = stems(title);
    let shared = |s: &&str| stems(s).iter().filter(|w| wanted.iter().any(|t| t.starts_with(w.as_str()) || w.starts_with(t.as_str()))).count();
    sentences(block).into_iter().map(|s| (shared(&s), s)).filter(|(n, _)| *n > 0).max_by_key(|(n, _)| *n).map_or(block, |(_, s)| s)
}

fn is_list_item(block: &str) -> bool {
    let t = block.trim_start();
    t.starts_with(['-', '*', '+', '•']) || t.split_once('.').is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The items of `extraction` that `block` (in `section`, see `section_of`) supports: no workout from
/// a program or plan, no workout weight the block does not state, no body weight that is a goal. A
/// list item is judged with its list; any other block on its own ("Sabah squat 5x5 100 kg yaptım"
/// beside "akşam sinemaya gideceğim" in one paragraph stays, final review I4).
pub fn guard(block: &str, section: &str, mut extraction: RawExtraction) -> RawExtraction {
    let context = if is_list_item(block) { section } else { block };
    let program = has_rep_range(context) || names_a_plan(context) || is_future(block);
    // a pasted program's lines and its heading ("Güç 2 (Pazartesi)") are no tasks either: the real
    // window made a task "Güç" for Monday
    let program_section = section.lines().any(is_list_item) && (has_rep_range(section) || names_a_plan(section));
    let weight_named = names_a_weight(block);
    extraction.items.retain_mut(|item| {
        let own = sentence_of(block, item.title.as_deref().unwrap_or_default());
        if let Some(when) = item.when.as_mut() {
            if !states_a_time(own) {
                when.time = None;
            }
            if when.repeat.is_some() && every_day(own) {
                when.repeat = Some(ALL_DAYS.to_vec());
            }
        }
        if item.kind != ItemType::Metric {
            return !program_section;
        }
        let Some(m) = item.metric.as_mut() else { return true };
        match m.kind {
            Some(MetricKind::Workout) if program => false,
            Some(MetricKind::Workout) => {
                if !weight_named {
                    m.value = None;
                    m.unit = None;
                }
                true
            }
            Some(MetricKind::Weight) => !is_goal_value(block, m.value),
            _ => true,
        }
    });
    extraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::parse_extraction;

    fn workout(value: &str) -> String {
        format!(r#"{{"items":[{{"type":"metric","metric":{{"kind":"workout","exercise":"bench press","sets":4,"reps":4,{value}}}}}]}}"#)
    }

    fn guard_one(text: &str, e: RawExtraction) -> RawExtraction {
        guard(text, text, e)
    }

    #[test]
    fn a_list_item_is_judged_with_its_list() {
        // the real window: "- Squat: 4x5" became a workout because its neighbour held the range
        let note = "Güç 1 (Pazar)
- Bench press: 4x4-6

Güç 2 (Pazartesi)
- Squat: 4x5
- Deadlift: 3-4x5 (RPE 8)
- Leg press: 4x5

Bugün bench press 4x6 60 kg yaptım.";
        let section = section_of(note, "- Squat: 4x5");
        assert!(section.starts_with("Güç 2") && section.contains("3-4x5"), "{section}");
        assert!(guard("- Squat: 4x5", section, parse_extraction(&workout(r#""value":0"#)).unwrap()).items.is_empty());
        let done = "Bugün bench press 4x6 60 kg yaptım.";
        assert_eq!(
            guard(done, section_of(note, done), parse_extraction(&workout(r#""value":60,"unit":"kg""#)).unwrap()).items.len(),
            1,
            "a log beside a program stays"
        );
    }

    #[test]
    fn a_program_gives_no_workouts() {
        // the owner's pasted program
        let text = "Güç 1 (Pazar)\n- Bench press: 4x4-6\n- Barbell row: 4x4-6";
        assert!(guard(text, text, parse_extraction(&workout(r#""value":6"#)).unwrap()).items.is_empty());
        for t in [
            "Deadlift: 3-4x5 (RPE 8)",
            "Leg curl: 3 x 8-10",
            "Antrenman planı:\nSquat 5x5",
            "Yarın bench press 4x6 80 kg deneyeceğim.",
            "Haftaya programım: pazartesi göğüs",
        ] {
            assert!(guard(t, t, parse_extraction(&workout(r#""value":80,"unit":"kg""#)).unwrap()).items.is_empty(), "{t}");
        }
    }

    #[test]
    fn a_workout_keeps_only_the_weight_the_note_states() {
        let kept = guard_one("Bugün bench press 4x4 60 kg yaptım", parse_extraction(&workout(r#""value":60,"unit":"kg""#)).unwrap());
        assert_eq!(kept.items[0].metric.as_ref().unwrap().value, Some(60.0));
        for t in ["bench press 4x4 60kg", "bench 4x4 60 kilo", "bench 4x4 at 135 lb"] {
            assert!(guard(t, t, parse_extraction(&workout(r#""value":60"#)).unwrap()).items[0].metric.as_ref().unwrap().value.is_some(), "{t}");
        }
        let dropped = guard_one("Bugün bench press 4x6 yaptım", parse_extraction(&workout(r#""value":6,"unit":"kg""#)).unwrap());
        let m = dropped.items[0].metric.as_ref().unwrap();
        assert_eq!((m.value, m.sets, m.reps), (None, Some(4), Some(4)), "sets and reps stay, the made-up weight goes");
    }

    #[test]
    fn a_goal_is_not_a_body_weight() {
        let weight = r#"{"items":[{"type":"metric","metric":{"kind":"weight","value":75,"unit":"kg"}}]}"#;
        assert!(guard_one("Hedefim 75 kiloya inmek", parse_extraction(weight).unwrap()).items.is_empty());
        assert!(guard_one("75 kilo olmak istiyorum", parse_extraction(weight).unwrap()).items.is_empty());
        assert_eq!(guard_one("Bugün 75 kiloyum", parse_extraction(weight).unwrap()).items.len(), 1);
    }

    #[test]
    fn the_review_cases_keep_real_workouts() {
        // final review I4: an exercise named like a plan, a log written with a dash, a future plan
        // elsewhere in the same paragraph
        let kept = |text: &str, value: &str| guard_one(text, parse_extraction(&workout(value)).unwrap()).items.len();
        assert_eq!(kept("plank 3x60 saniye yaptım", r#""value":0"#), 1);
        assert_eq!(kept("Bulgarian split squat 3x10 20 kg", r#""value":20,"unit":"kg""#), 1);
        assert_eq!(kept("Bench 3x10 - 60 kg", r#""value":60,"unit":"kg""#), 1);
        let day = "Sabah squat 5x5 100 kg yaptım.
Akşam sinemaya gideceğim.";
        let block = "Sabah squat 5x5 100 kg yaptım.
Akşam sinemaya gideceğim.";
        assert_eq!(
            guard(block, section_of(day, block), parse_extraction(&workout(r#""value":100,"unit":"kg""#)).unwrap()).items.len(),
            1,
            "done today, whatever is planned for tonight"
        );
        let log = "Sabah squat 5x5 100 kg yaptım.";
        assert_eq!(
            guard(
                log,
                section_of(
                    "Sabah squat 5x5 100 kg yaptım.

Akşam sinemaya gideceğim.",
                    log
                ),
                parse_extraction(&workout(r#""value":100,"unit":"kg""#)).unwrap()
            )
            .items
            .len(),
            1
        );
        let km = guard_one("5 kilometre koştum, bench 3x10", parse_extraction(&workout(r#""value":40,"unit":"kg""#)).unwrap());
        assert_eq!(km.items[0].metric.as_ref().unwrap().value, None, "kilometres are not kilograms (M6)");
    }

    #[test]
    fn a_weight_beside_a_goal_stays() {
        let weight = |v: &str| format!(r#"{{"items":[{{"type":"metric","metric":{{"kind":"weight","value":{v},"unit":"kg"}}}}]}}"#);
        assert_eq!(guard_one("bugün 82 kiloyum, 75 olmak istiyorum", parse_extraction(&weight("82")).unwrap()).items.len(), 1);
        assert!(guard_one("bugün 82 kiloyum, 75 olmak istiyorum", parse_extraction(&weight("75")).unwrap()).items.is_empty());
        assert_eq!(guard_one("82,5 kiloyum", parse_extraction(&weight("82.5")).unwrap()).items.len(), 1);
    }

    #[test]
    fn a_section_is_found_in_a_note_with_windows_line_ends() {
        // final review M5
        let note = "Güç 2
- Squat: 4x5
- Deadlift: 3-4x5

Başka
";
        assert!(section_of(note, "- Squat: 4x5").contains("3-4x5"));
    }

    #[test]
    fn a_program_heading_is_no_task() {
        let note = "Güç 2 (Pazartesi)
- Squat: 4x5
- Deadlift: 3-4x5

Pazartesi dişçi var.";
        let task = r#"{"items":[{"type":"task","title":"Güç","when":{"weekday":"mon","which":"this"}}]}"#;
        assert!(guard("Güç 2 (Pazartesi)", section_of(note, "Güç 2 (Pazartesi)"), parse_extraction(task).unwrap()).items.is_empty());
        let dentist = r#"{"items":[{"type":"task","title":"Dişçi","when":{"weekday":"mon","which":"this"}}]}"#;
        assert_eq!(
            guard("Pazartesi dişçi var.", section_of(note, "Pazartesi dişçi var."), parse_extraction(dentist).unwrap()).items.len(),
            1,
            "a task beside it stays"
        );
        let todo = "Yapılacaklar
- Faturayı öde
- Annemi ara";
        let pay = r#"{"items":[{"type":"task","title":"Faturayı öde"}]}"#;
        assert_eq!(guard("- Faturayı öde", section_of(todo, "- Faturayı öde"), parse_extraction(pay).unwrap()).items.len(), 1, "a to-do list is no program");
    }

    #[test]
    fn a_weekly_habit_passes_the_guard() {
        let habit = r#"{"items":[{"type":"task","title":"Spor","when":{"repeat":["sun","mon","wed","thu","fri"],"time":"20:00"}}]}"#;
        let e = guard_one("Her pazar pazartesi çarşamba perşembe cuma akşam 8'de spora gideceğim.", parse_extraction(habit).unwrap());
        assert_eq!(e.items.len(), 1);
    }

    #[test]
    fn a_time_the_block_does_not_state_goes() {
        // the owner's report: "yatmadan" became 22:00
        let habit = r#"{"items":[{"type":"task","title":"Kelime tekrarı","when":{"repeat":["mon"],"time":"22:00"}}]}"#;
        let e = guard_one("Her akşam yatmadan ingilizce kelime tekrarı yapacağım.", parse_extraction(habit).unwrap());
        assert_eq!(e.items[0].when.as_ref().unwrap().time, None);
        assert!(e.items[0].when.as_ref().unwrap().repeat.is_some(), "the days stay");
        // two habits in one paragraph: each item is judged by its own sentence
        let both = r#"{"items":[{"type":"task","title":"Spor","when":{"repeat":["mon"],"time":"20:00"}},{"type":"task","title":"İngilizce kelime tekrarı","when":{"repeat":["mon"],"time":"20:00"}}]}"#;
        for text in [
            "Her pazar pazartesi çarşamba perşembe cuma akşam 8 de spora gideceğim. Her akşam yatmadan ingilizce kelime tekrarı yapacağım.",
            "Her pazar pazartesi çarşamba perşembe cuma akşam 8 de spora gideceğim.Her akşam yatmadan ingilizce kelime tekrarı yapacağım.",
        ] {
            let e = guard_one(text, parse_extraction(both).unwrap());
            let times: Vec<Option<String>> = e.items.iter().map(|i| i.when.as_ref().unwrap().time.clone()).collect();
            assert_eq!(times, [Some("20:00".to_owned()), None], "{text}");
        }
        let half = r#"{"items":[{"type":"task","title":"Toplantı","when":{"day_offset":1,"time":"07:30"}}]}"#;
        assert!(guard_one("Yarın 7.30 toplantı. Sonra kahve.", parse_extraction(half).unwrap()).items[0].when.as_ref().unwrap().time.is_some());
        let at = |text: &str, time: &str| {
            let task = format!(r#"{{"items":[{{"type":"task","title":"x","when":{{"day_offset":1,"time":"{time}"}}}}]}}"#);
            guard_one(text, parse_extraction(&task).unwrap()).items[0].when.as_ref().unwrap().time.clone()
        };
        assert_eq!(at("yarın akşam 8 de spor", "20:00").as_deref(), Some("20:00"));
        assert_eq!(at("yarın öğlen yemek", "12:00").as_deref(), Some("12:00"), "a time word is a time");
        assert_eq!(at("lunch at noon tomorrow", "12:00").as_deref(), Some("12:00"));
        assert_eq!(at("yarın gece yarısı uçuş", "00:00").as_deref(), Some("00:00"));
        assert_eq!(at("call Ali at midnight", "00:00").as_deref(), Some("00:00"));
        assert_eq!(at("yarın sabah koşu", "07:00"), None);
    }

    #[test]
    fn a_habit_keeps_the_days_of_its_own_sentence() {
        // the model's answer in the owner's report: the second habit copied the first one's days
        let raw = r#"{"items":[{"type":"task","title":"Spor","when":{"repeat":["sun","mon","wed","thu","fri"],"time":"20:00"}},{"type":"task","title":"Kelime tekrarı","when":{"repeat":["sun","mon","wed","thu","fri"],"time":"22:00"}}]}"#;
        let text = "Her pazar pazartesi çarşamba perşembe cuma akşam 8 de spora gideceğim.Her akşam yatmadan ingilizce kelime tekrarı yapacağım.";
        let e = guard_one(text, parse_extraction(raw).unwrap());
        let days = |i: usize| e.items[i].when.as_ref().unwrap().repeat.clone().unwrap();
        use crate::extraction::Weekday::*;
        assert_eq!(days(0), [Sun, Mon, Wed, Thu, Fri], "named days stay");
        assert_eq!(days(1), [Mon, Tue, Wed, Thu, Fri, Sat, Sun], "\"her akşam\" is every day");
        // a sentence naming days, a weekend or no "her" keeps what the model wrote
        let one = |text: &str| {
            let habit = r#"{"items":[{"type":"task","title":"Kitap","when":{"repeat":["sat","sun"]}}]}"#;
            guard_one(text, parse_extraction(habit).unwrap()).items[0].when.as_ref().unwrap().repeat.clone().unwrap().len()
        };
        assert_eq!(one("Hafta sonu her sabah kitap okuyacağım."), 2);
        assert_eq!(one("Cumartesileri ve pazarları kitap okuyacağım."), 2);
        assert_eq!(one("Every weekend I will read a book."), 2);
        assert_eq!(one("Her gün kitap okuyacağım."), 7);
        assert_eq!(one("I will read a book every evening."), 7);
    }

    #[test]
    fn tasks_and_other_measurements_pass() {
        let mixed = r#"{"items":[{"type":"task","title":"Dişçi"},{"type":"metric","metric":{"kind":"sleep","value":7,"unit":"h"}}]}"#;
        assert_eq!(guard_one("Yarın dişçi. Programım yoğun, 7 saat uyudum.", parse_extraction(mixed).unwrap()).items.len(), 2);
    }
}
