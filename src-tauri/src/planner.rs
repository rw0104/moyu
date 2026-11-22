use crate::models::TranslationPlan;

// Operational per-request budget, not a limit on the user's document.
// UTF-8 bytes are a deliberately conservative estimate, not the provider tokenizer.
pub const OUTPUT_TOKEN_BUDGET: usize = 32_768;
const OUTPUT_HEADROOM: usize = 2_048;
pub const SEGMENT_BYTE_BUDGET: usize = (OUTPUT_TOKEN_BUDGET - OUTPUT_HEADROOM) / 2;

pub fn output_budget(text: &str) -> usize {
    text.len()
        .saturating_mul(2)
        .saturating_add(OUTPUT_HEADROOM)
        .clamp(8_192, OUTPUT_TOKEN_BUDGET)
}

pub fn bisect(text: &str) -> Option<(String, String)> {
    let mut middle = text.len() / 2;
    while middle > 0 && !text.is_char_boundary(middle) {
        middle -= 1;
    }
    if middle == 0 {
        middle = text.char_indices().nth(1)?.0;
    }
    if middle >= text.len() {
        return None;
    }
    Some((text[..middle].to_owned(), text[middle..].to_owned()))
}

pub fn plan(text: &str) -> TranslationPlan {
    let mut segments = Vec::new();
    let mut remainder = text;
    while !remainder.is_empty() {
        if remainder.len() <= SEGMENT_BYTE_BUDGET {
            segments.push(remainder.to_owned());
            break;
        }
        let mut end = SEGMENT_BYTE_BUDGET;
        while !remainder.is_char_boundary(end) {
            end -= 1;
        }
        let prefix = &remainder[..end];
        let minimum = end / 2;
        // Prefer a complete paragraph, line, sentence, then word. Keep delimiters.
        let paragraph = prefix.rfind("\n\n").map(|index| index + 2);
        let line = prefix.rfind('\n').map(|index| index + 1);
        let sentence = prefix
            .char_indices()
            .rev()
            .find(|(_, ch)| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？' | ';' | '；'))
            .map(|(index, ch)| index + ch.len_utf8());
        let word = prefix
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(index, ch)| index + ch.len_utf8());
        end = [paragraph, line, sentence, word]
            .into_iter()
            .flatten()
            .find(|index| *index >= minimum)
            .unwrap_or(end);
        // Keep Windows line endings together when the boundary falls between them.
        if remainder.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\r')
            && remainder.as_bytes().get(end) == Some(&b'\n')
        {
            end -= 1;
        }
        segments.push(remainder[..end].to_owned());
        remainder = &remainder[end..];
    }
    TranslationPlan {
        segments,
        estimated_input_tokens: text.len(),
        output_token_budget: OUTPUT_TOKEN_BUDGET,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_long_english_document_is_not_blocked_or_split_unnecessarily() {
        let text = "a".repeat(7_680);
        let planned = plan(&text);
        assert_eq!(planned.segments, vec![text]);
        assert!(output_budget(&planned.segments[0]) > 16_384);
    }
    #[test]
    fn arbitrary_length_and_unicode_are_preserved_exactly() {
        for text in [
            "word ".repeat(14_000),
            "中文😀e\u{301}\r\n\r\n".repeat(10_000),
            format!("{}hello{}", " ".repeat(20_000), "\n".repeat(20_000)),
        ] {
            let planned = plan(&text);
            assert_eq!(planned.segments.concat(), text);
            assert!(planned
                .segments
                .iter()
                .all(|segment| !segment.is_empty() && segment.len() <= SEGMENT_BYTE_BUDGET));
        }
    }
    #[test]
    fn preserves_paragraph_boundary_and_handles_unbroken_words() {
        let paragraph = format!("{}\n\n{}", "a".repeat(10_000), "b".repeat(10_000));
        assert!(plan(&paragraph).segments[0].ends_with("\n\n"));
        let word = "x".repeat(100_000);
        assert_eq!(plan(&word).segments.concat(), word);
    }
    #[test]
    fn truncation_fallback_bisects_without_losing_unicode() {
        for text in ["abc", "😀你好\r\n世界", " e\u{301} mixed"] {
            let (left, right) = bisect(text).unwrap();
            assert!(!left.is_empty() && !right.is_empty());
            assert_eq!(format!("{left}{right}"), text);
        }
        assert!(bisect("😀").is_none());
    }
}
