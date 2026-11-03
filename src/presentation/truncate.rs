use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Default ellipsis appended when text is truncated.
pub const DEFAULT_ELLIPSIS: &str = "...";

/// Truncates a string to fit within a specified maximum terminal column display width.
///
/// If the text exceeds `max_width`, it is truncated and `ellipsis` is appended such
/// that the total visual display width of the resulting string does not exceed `max_width`.
///
/// # Guarantees:
/// 1. **Panic-Free**: Never panics on any valid UTF-8 string, regardless of byte boundaries.
/// 2. **Grapheme-Cluster Safe**: Never splits a Unicode scalar value, surrogate pair,
///    combining accent, or multi-codepoint grapheme cluster (such as flags or ZWJ emoji).
/// 3. **Display-Width Aware**: Distinguishes byte length and character count from terminal
///    column width (e.g. CJK ideographs and emoji occupy 2 columns, combining accents occupy 0).
/// 4. **Boundary Preserving**: If truncation falls in the middle of a multi-column or
///    multi-byte character, the character is omitted rather than corrupted.
///
/// # Edge Cases:
/// - Empty string `""`: returns `""`.
/// - `max_width == 0`: returns `""`.
/// - Text fits within `max_width`: returns the original text without appending an ellipsis.
/// - `max_width < ellipsis.width()`: returns the portion of the ellipsis that fits within `max_width`.
pub fn truncate_to_width(text: &str, max_width: usize, ellipsis: &str) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = UnicodeWidthStr::width(text);
    if text_width <= max_width {
        return text.to_string();
    }

    let ellipsis_width = UnicodeWidthStr::width(ellipsis);

    // If max_width cannot even fit the full ellipsis, take as many graphemes of the ellipsis as fit
    if max_width < ellipsis_width {
        let mut accumulated_width = 0;
        let mut result = String::new();
        for g in ellipsis.graphemes(true) {
            let gw = UnicodeWidthStr::width(g);
            if accumulated_width + gw > max_width {
                break;
            }
            result.push_str(g);
            accumulated_width += gw;
        }
        return result;
    }

    let available_content_width = max_width - ellipsis_width;
    let mut accumulated_width = 0;
    let mut result = String::new();

    for g in text.graphemes(true) {
        let gw = UnicodeWidthStr::width(g);
        if accumulated_width + gw > available_content_width {
            break;
        }
        result.push_str(g);
        accumulated_width += gw;
    }

    result.push_str(ellipsis);
    result
}

/// Convenience wrapper that truncates `text` to fit within `max_width` terminal columns
/// using the standard `"..."` ellipsis.
pub fn truncate_str(text: &str, max_width: usize) -> String {
    truncate_to_width(text, max_width, DEFAULT_ELLIPSIS)
}

/// Truncates a string based on user-perceived grapheme cluster count rather than terminal column width.
///
/// Suitable when display width is uniform or unconstrained, but character count needs bounding.
pub fn truncate_to_graphemes(text: &str, max_graphemes: usize, ellipsis: &str) -> String {
    if max_graphemes == 0 {
        return String::new();
    }

    let total_graphemes = text.graphemes(true).count();
    if total_graphemes <= max_graphemes {
        return text.to_string();
    }

    let ellipsis_graphemes = ellipsis.graphemes(true).count();
    if max_graphemes < ellipsis_graphemes {
        return ellipsis.graphemes(true).take(max_graphemes).collect();
    }

    let take_count = max_graphemes - ellipsis_graphemes;
    let mut result: String = text.graphemes(true).take(take_count).collect();
    result.push_str(ellipsis);
    result
}
