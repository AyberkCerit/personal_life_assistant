//! FR-EXT-006: a re-processed note's blocks are matched to the stored ones by text similarity,
//! position breaking ties, so an edited paragraph keeps its `block_id` and its items.

/// Longer texts are compared on this many leading characters (keeps matching linear in practice).
const MAX_COMPARED_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq)]
pub struct OldBlock {
    pub id: String,
    pub position: usize,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockMatch {
    pub matched: Vec<(usize, String)>,
    pub unmatched_new: Vec<usize>,
    pub unmatched_old: Vec<String>,
}

pub fn similarity(a: &str, b: &str) -> f64 {
    let a: String = a.chars().take(MAX_COMPARED_CHARS).collect();
    let b: String = b.chars().take(MAX_COMPARED_CHARS).collect();
    strsim::normalized_levenshtein(&a, &b)
}

pub fn match_blocks(old: &[OldBlock], new: &[&str], threshold: f64) -> BlockMatch {
    let mut old_used = vec![false; old.len()];
    let mut new_match: Vec<Option<usize>> = vec![None; new.len()];
    let distance = |i: usize, j: usize| i.abs_diff(old[j].position);

    // 1. identical text, nearest position first
    for (i, text) in new.iter().enumerate() {
        let best = (0..old.len()).filter(|&j| !old_used[j] && old[j].text == *text).min_by_key(|&j| distance(i, j));
        if let Some(j) = best {
            old_used[j] = true;
            new_match[i] = Some(j);
        }
    }

    // 2. similar text: best score first, then nearest position
    let mut candidates = Vec::new();
    for (i, text) in new.iter().enumerate().filter(|(i, _)| new_match[*i].is_none()) {
        for j in (0..old.len()).filter(|&j| !old_used[j]) {
            let (la, lb) = (text.chars().count(), old[j].text.chars().count());
            if la.min(lb) as f64 / la.max(lb).max(1) as f64 >= threshold {
                let score = similarity(text, &old[j].text);
                if score >= threshold {
                    candidates.push((score, distance(i, j), i, j));
                }
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, _, i, j) in candidates {
        if new_match[i].is_none() && !old_used[j] {
            old_used[j] = true;
            new_match[i] = Some(j);
        }
    }

    let mut out = BlockMatch::default();
    for (i, m) in new_match.into_iter().enumerate() {
        match m {
            Some(j) => out.matched.push((i, old[j].id.clone())),
            None => out.unmatched_new.push(i),
        }
    }
    out.unmatched_old = (0..old.len()).filter(|&j| !old_used[j]).map(|j| old[j].id.clone()).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old(blocks: &[(&str, usize, &str)]) -> Vec<OldBlock> {
        blocks.iter().map(|(id, p, t)| OldBlock { id: (*id).into(), position: *p, text: (*t).into() }).collect()
    }

    #[test]
    fn identical_text_matches_even_after_reordering() {
        let o = old(&[("a", 0, "Yarın 9'da dişçi"), ("b", 1, "Akşam koşu")]);
        let m = match_blocks(&o, &["Akşam koşu", "Yarın 9'da dişçi"], 0.6);
        assert_eq!(m.matched, vec![(0, "b".to_string()), (1, "a".to_string())]);
        assert!(m.unmatched_new.is_empty() && m.unmatched_old.is_empty());
    }

    #[test]
    fn a_typo_fix_keeps_the_block() {
        // Review Focus 1
        let o = old(&[("a", 0, "Yarın 9da dişci var")]);
        let m = match_blocks(&o, &["Yarın 9'da dişçi var"], 0.6);
        assert_eq!(m.matched, vec![(0, "a".to_string())]);
    }

    #[test]
    fn unrelated_text_is_a_new_block_and_the_old_one_is_gone() {
        let o = old(&[("a", 0, "Yarın 9'da dişçi")]);
        let m = match_blocks(&o, &["Cuma günü fatura öde"], 0.6);
        assert!(m.matched.is_empty());
        assert_eq!(m.unmatched_new, vec![0]);
        assert_eq!(m.unmatched_old, vec!["a".to_string()]);
    }

    #[test]
    fn duplicates_pair_by_nearest_position() {
        let o = old(&[("a", 0, "- su iç"), ("b", 5, "- su iç")]);
        let m = match_blocks(&o, &["başlık", "x", "y", "z", "w", "- su iç"], 0.6);
        assert_eq!(m.matched, vec![(5, "b".to_string())]);
        assert_eq!(m.unmatched_old, vec!["a".to_string()]);
    }

    #[test]
    fn similarity_bounds() {
        assert_eq!(similarity("aynı", "aynı"), 1.0);
        assert!(similarity("Yarın dişçi", "Cuma fatura") < 0.6);
        assert_eq!(similarity("", ""), 1.0);
    }
}
