//! Label line splitting and line height measurement.

/// Line height multiplier applied to the font size for stacked label lines.
pub const LABEL_LINE_HEIGHT_SCALE: f32 = 1.25;

/// Maximum characters per label line before hard wrapping.
///
/// Wrapping cuts at character boundaries rather than spaces so CJK text
/// without word separators still breaks; Latin words may split mid-word,
/// which is acceptable without rich text support.
pub const MAX_LABEL_CHARS_PER_LINE: usize = 24;

/// Height of one label line for `size`, used to stack wrapped lines.
pub fn line_height(size: f32) -> f32 {
    size.max(1.0) * LABEL_LINE_HEIGHT_SCALE
}

/// Splits label text into drawable lines.
///
/// Lines break on explicit newlines first, then overlong lines hard-wrap at
/// character boundaries. Blank lines are preserved as empty entries so the
/// vertical rhythm stays aligned with the source text.
pub fn split_label_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        wrap_label_line(line, &mut out);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Pushes one newline-delimited line, wrapping it when overlong.
fn wrap_label_line(line: &str, out: &mut Vec<String>) {
    if line.chars().count() <= MAX_LABEL_CHARS_PER_LINE {
        out.push(line.to_string());
        return;
    }
    let mut current = String::new();
    let mut count = 0usize;
    for glyph in line.chars() {
        current.push(glyph);
        count += 1;
        if count >= MAX_LABEL_CHARS_PER_LINE {
            out.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_text_splits_and_wraps_without_panicking() {
        assert_eq!(split_label_lines("hello"), vec!["hello".to_string()]);
        assert_eq!(
            split_label_lines("你好\n世界"),
            vec!["你好".to_string(), "世界".to_string()]
        );
        assert_eq!(
            split_label_lines("a\n\nb"),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
        let long: String = "中".repeat(MAX_LABEL_CHARS_PER_LINE * 2 + 2);
        let wrapped = split_label_lines(&long);
        assert_eq!(wrapped.len(), 3);
        assert!(
            wrapped
                .iter()
                .all(|line| line.chars().count() <= MAX_LABEL_CHARS_PER_LINE)
        );
        assert_eq!(wrapped.concat(), long);
        assert_eq!(line_height(12.0), 15.0);
        assert_eq!(line_height(0.0), LABEL_LINE_HEIGHT_SCALE);
    }
}
