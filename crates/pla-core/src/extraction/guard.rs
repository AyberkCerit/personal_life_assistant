//! Checks on the model's items that need the note's own words (nl-quality plan § 2). The model
//! read a pasted workout program ("Bench press: 4x4-6") as four workouts done today at 6 kg.

use super::{ItemType, MetricKind, RawExtraction};

/// "4x4-6", "3-4x5", "3x8-10": a range of sets or repetitions belongs to a program, not to a
/// workout someone did (they did one number).
fn has_rep_range(text: &str) -> bool {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let num_end = |mut i: usize| {
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let skip_space = |mut i: usize| {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        // number, then [x×*] number, then [-–] number; or number [-–] number [x×] number
        let a = num_end(i);
        let b = skip_space(a);
        if b < chars.len() && "x×*".contains(chars[b]) {
            let c = skip_space(b + 1);
            let d = num_end(c);
            let e = skip_space(d);
            if d > c && e < chars.len() && "-–".contains(chars[e]) && num_end(skip_space(e + 1)) > skip_space(e + 1) {
                return true;
            }
        }
        if b < chars.len() && "-–".contains(chars[b]) {
            let c = skip_space(b + 1);
            let d = num_end(c);
            let e = skip_space(d);
            if d > c && e < chars.len() && "x×".contains(chars[e]) && num_end(skip_space(e + 1)) > skip_space(e + 1) {
                return true;
            }
        }
        i = a.max(i + 1);
    }
    false
}

fn words(text: &str) -> Vec<String> {
    // Turkish letters folded too: people type "deneyecegim" as often as "deneyeceğim"
    let plain: String = crate::index::key(text)
        .chars()
        .map(|c| match c {
            'ç' => 'c',
            'ğ' => 'g',
            'ş' => 's',
            'ö' => 'o',
            'ü' => 'u',
            c => c,
        })
        .collect();
    plain.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// A plan or program rather than a log: "program", "plan", "split", or a workout still to come.
fn is_plan(text: &str) -> bool {
    words(text).iter().any(|w| {
        ["program", "plan", "split", "routine", "rutin"].iter().any(|p| w.starts_with(p))
            || w.ends_with("acagim")
            || w.ends_with("ecegim")
            || w.ends_with("acak")
            || w.ends_with("ecek")
            || w == "will"
            || w == "haftaya"
    })
}

/// Whether the note gives a weight unit at all; without one a workout's number is not kilograms.
fn names_a_weight(text: &str) -> bool {
    words(text).iter().any(|w| {
        w.starts_with("kg") || w.starts_with("kilo") || w == "lb" || w == "lbs" || w.starts_with("pound") || w.starts_with("libre")
            || (w.ends_with("kg") && w.trim_end_matches("kg").chars().all(|c| c.is_ascii_digit() || c == '.' || c == ','))
    })
}

/// A goal or a wish: "75 kiloya inmek istiyorum", "my goal is 70 kg".
fn is_goal(text: &str) -> bool {
    words(text).iter().any(|w| w.starts_with("hedef") || w.starts_with("istiyor") || w == "goal" || w == "target" || w == "want" || w.starts_with("olmak"))
}

/// The part of `note` a block belongs to: the lines between blank lines around it. A list item is a
/// block of its own, so "- Squat: 4x5" alone does not show that "- Deadlift: 3-4x5" next to it
/// makes the whole list a program (seen in the real window).
pub fn section_of<'a>(note: &'a str, block: &'a str) -> &'a str {
    let block = block.trim();
    // CRLF notes too: a blank line is any line with nothing but spaces
    let mut sections: Vec<String> = vec![String::new()];
    for line in note.lines() {
        if line.trim().is_empty() {
            sections.push(String::new());
        } else {
            let s = sections.last_mut().expect("one section at least");
            s.push_str(line);
            s.push('\n');
        }
    }
    let found = sections.iter().position(|s| !block.is_empty() && s.contains(block));
    match found {
        // a slice of `note` is not at hand once lines are joined; find the same text in it
        Some(i) => note.find(sections[i].lines().next().unwrap_or_default()).map_or(block, |start| {
            let end = sections[i].lines().last().and_then(|l| note[start..].find(l).map(|e| start + e + l.len())).unwrap_or(note.len());
            &note[start..end]
        }),
        None => block,
    }
}

/// The items of `extraction` that `block` (in `section`, see `section_of`) supports: no workout from a
/// program or plan, no workout weight the block does not state, no body weight from a goal.
pub fn guard(block: &str, section: &str, mut extraction: RawExtraction) -> RawExtraction {
    let program = has_rep_range(section) || is_plan(section) || has_rep_range(block) || is_plan(block);
    let weight_named = names_a_weight(block);
    let goal = is_goal(block);
    extraction.items.retain_mut(|item| {
        let Some(m) = item.metric.as_mut().filter(|_| item.kind == ItemType::Metric) else { return true };
        match m.kind {
            Some(MetricKind::Workout) if program => false,
            Some(MetricKind::Workout) => {
                if !weight_named {
                    m.value = None;
                    m.unit = None;
                }
                true
            }
            Some(MetricKind::Weight) => !goal,
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
        assert_eq!(guard(done, section_of(note, done), parse_extraction(&workout(r#""value":60,"unit":"kg""#)).unwrap()).items.len(), 1, "a log beside a program stays");
    }

    #[test]
    fn a_program_gives_no_workouts() {
        // the owner's pasted program
        let text = "Güç 1 (Pazar)\n- Bench press: 4x4-6\n- Barbell row: 4x4-6";
        assert!(guard(text, text, parse_extraction(&workout(r#""value":6"#)).unwrap()).items.is_empty());
        for t in ["Deadlift: 3-4x5 (RPE 8)", "Leg curl: 3 x 8-10", "Antrenman planı:\nSquat 5x5", "Yarın bench press 4x6 80 kg deneyeceğim.", "Haftaya programım: pazartesi göğüs"] {
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
    fn tasks_and_other_measurements_pass() {
        let mixed = r#"{"items":[{"type":"task","title":"Dişçi"},{"type":"metric","metric":{"kind":"sleep","value":7,"unit":"h"}}]}"#;
        assert_eq!(guard_one("Yarın dişçi. Programım yoğun, 7 saat uyudum.", parse_extraction(mixed).unwrap()).items.len(), 2);
    }
}
