//! 共享行内分词：把一行 OCR 文本切成词级 token（macOS Vision 与 Paddle 共用）。

#[derive(Debug)]
pub(crate) struct LineToken {
    pub text: String,
    /// token 首字符在行文本中的 char 索引
    pub char_start: usize,
    /// token 的 char 数
    pub char_len: usize,
}

/// 把一行 OCR 文本切成 token：CJK 字符逐字成 token；拉丁/数字连续串成一个
/// token；空白分隔。规则与原 vision.rs::macos_ocr_tokens 一致（含韩文范围）。
pub(crate) fn split_line_tokens(text: &str) -> Vec<LineToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = 0_usize;
    let mut char_offset = 0_usize;
    let mut current_is_cjk = false;

    for char in text.chars() {
        if char.is_whitespace() {
            push_line_token(&mut tokens, &mut current, current_start, char_offset);
            char_offset += 1;
            current_is_cjk = false;
            continue;
        }

        let is_cjk = is_cjk_char(char);
        if current.is_empty() {
            current_start = char_offset;
            current_is_cjk = is_cjk;
        } else if is_cjk || current_is_cjk {
            push_line_token(&mut tokens, &mut current, current_start, char_offset);
            current_start = char_offset;
            current_is_cjk = is_cjk;
        }

        current.push(char);
        char_offset += 1;

        if is_cjk {
            push_line_token(&mut tokens, &mut current, current_start, char_offset);
            current_is_cjk = false;
        }
    }

    push_line_token(&mut tokens, &mut current, current_start, char_offset);
    tokens
}

fn push_line_token(tokens: &mut Vec<LineToken>, current: &mut String, start: usize, end: usize) {
    let value = current.trim();
    if !value.is_empty() && end > start {
        tokens.push(LineToken {
            text: value.to_string(),
            char_start: start,
            char_len: end - start,
        });
    }
    current.clear();
}

/// char 索引 → utf16 code unit 偏移（vision.rs 适配 Vision NSRange 用）。
/// 仅 macOS Vision 管线调用，非 macOS 非测试构建无调用方，
/// allow(dead_code) 消除平台性警告。
#[allow(dead_code)]
pub(crate) fn char_index_to_utf16(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).map(char::len_utf16).sum()
}

fn is_cjk_char(char: char) -> bool {
    matches!(
        char as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_line_tokens_cjk_each_char_is_token() {
        let tokens = split_line_tokens("你好");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["你", "好"]);
        assert_eq!(tokens[0].char_start, 0);
        assert_eq!(tokens[1].char_start, 1);
    }

    #[test]
    fn split_line_tokens_latin_word_is_single_token() {
        let tokens = split_line_tokens("hello world");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["hello", "world"]);
        assert_eq!(tokens[1].char_start, 6);
        assert_eq!(tokens[0].char_len, 5);
        assert_eq!(tokens[1].char_len, 5);
    }

    #[test]
    fn split_line_tokens_mixed_cjk_and_latin() {
        let tokens = split_line_tokens("金额123元");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["金", "额", "123", "元"]);
    }

    #[test]
    fn split_line_tokens_skips_whitespace_only_input() {
        assert!(split_line_tokens("   ").is_empty());
        assert!(split_line_tokens("").is_empty());
    }

    #[test]
    fn split_line_tokens_korean_each_syllable_is_token() {
        let tokens = split_line_tokens("한국어");
        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn char_index_to_utf16_bmp_is_identity() {
        assert_eq!(char_index_to_utf16("abc", 2), 2);
    }

    #[test]
    fn char_index_to_utf16_counts_surrogate_pairs_as_two() {
        // U+1F600 占 2 个 utf16 单元
        assert_eq!(char_index_to_utf16("😀x", 1), 2);
    }
}
