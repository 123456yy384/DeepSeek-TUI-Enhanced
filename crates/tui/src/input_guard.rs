//! Input sanitization — strip known special tokens from user input.
//!
//! Strategy: delete `<think>`, `<|User|>`, `<BOS>`, etc. from the input
//! text.  If nothing meaningful remains, the caller inserts an assistant
//! message asking the user what they need.

const SPECIAL_TOKENS: &[&str] = &[
    "<!--", "-->",
    "</think>", "<think>", "</thinking>", "<thinking>",
    "</reasoning>", "<reasoning>", "</answer>", "<answer>",
    "<BOS>", "<EOS>", "<bos>", "<eos>", "<s>", "</s>",
    "[BOS]", "[EOS]",
    "<pad>", "<unk>", "<mask>", "<sep>", "<cls>",
    "system", "user", "assistant",
];

/// Result after stripping special tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanitizedInput {
    /// Meaningful content remains (possibly empty if input was all tokens).
    Clean(String),
}

/// Strip known special tokens. Returns the cleaned text.
pub fn sanitize_user_input(raw: &str) -> SanitizedInput {
    let mut result = raw.trim().to_string();
    for token in SPECIAL_TOKENS {
        result = result.replace(token, "");
    }
    // Also strip any remaining `<|...|>` patterns (pipe-delimited ChatML)
    result = strip_pipe_tokens(&result);
    // Strip any remaining `<...>` patterns that have no spaces (bare control tokens)
    result = strip_bare_tokens(&result);
    SanitizedInput::Clean(result.trim().to_string())
}

fn strip_pipe_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
            // Find closing `>`
            if let Some(j) = text[i..].find('>') {
                i += j + 1;
                continue;
            }
        }
        // Preserve multi-byte UTF-8
        let ch = text[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn strip_bare_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(end) = text[i..].find('>') {
                let inner = &text[i + 1..i + end];
                // Strip if: no spaces, no letters (pure control token)
                if !inner.contains(' ')
                    && !inner.chars().any(|c| c.is_alphabetic())
                    && !inner.contains('\u{200B}')
                {
                    i += end + 1;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_think_stripped() {
        let SanitizedInput::Clean(s) = sanitize_user_input("<think>");
        assert_eq!(s, "");
    }

    #[test]
    fn complex_injection_stripped() {
        let SanitizedInput::Clean(s) =
            sanitize_user_input("<|begin_of_sentence|><|sft__begin| ><think>");
        assert_eq!(s, "");
    }

    #[test]
    fn normal_text_unchanged() {
        let SanitizedInput::Clean(s) = sanitize_user_input("hello");
        assert_eq!(s, "hello");
    }

    #[test]
    fn short_ok_unchanged() {
        let SanitizedInput::Clean(s) = sanitize_user_input("OK");
        assert_eq!(s, "OK");
    }

    #[test]
    fn short_chinese_unchanged() {
        let SanitizedInput::Clean(s) = sanitize_user_input("好了");
        assert_eq!(s, "好了");
    }

    #[test]
    fn token_in_real_text_stripped() {
        let SanitizedInput::Clean(s) = sanitize_user_input("what is <think>?");
        assert_eq!(s, "what is ?");
    }

    #[test]
    fn digit_with_think_stripped() {
        let SanitizedInput::Clean(s) = sanitize_user_input("1<think>");
        assert_eq!(s, "1");
    }

    #[test]
    fn html_passes_through() {
        let SanitizedInput::Clean(s) =
            sanitize_user_input("<div class='foo'>hello</div>");
        assert!(s.contains("<div class='foo'>"));
        assert!(s.contains("hello"));
    }
}
