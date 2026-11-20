use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Default ellipsis appended when text is truncated.
pub const DEFAULT_ELLIPSIS: &str = "...";

/// Returns true if a character is a Unicode BiDi control or zero-width formatting character (H-23).
pub fn is_bidi_or_formatting_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO
        | '\u{2066}'..='\u{2069}' // LRI, RLI, FSI, PDI
        | '\u{200E}'..='\u{200F}' // LRM, RLM
        | '\u{061C}'              // ALM
        | '\u{200B}'              // Zero-width space
        | '\u{FEFF}'              // BOM / Zero-width no-break space
    )
}

/// Escapes or sanitizes terminal control sequences, ANSI/OSC codes, C0/C1 control characters,
/// newlines, and BiDi formatting overrides to prevent terminal injection and prompt spoofing (H-23).
pub fn escape_terminal_safe(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // ANSI / VT100 / OSC Escape sequence start
            '\x1b' => {
                // If followed by '[' (CSI sequence), consume up to final command byte (0x40..=0x7E)
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if (0x40..=0x7e).contains(&(next as u32)) {
                            break;
                        }
                    }
                } else if chars.peek() == Some(&']') {
                    // OSC sequence: consume until BEL (\x07) or ST (\x1b\)
                    chars.next(); // consume ']'
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                } else {
                    // Lone escape character - escape as printable literal
                    out.push_str(r"\x1b");
                }
            }
            // Newlines and carriage returns: convert to space to prevent terminal line spoofing
            '\r' | '\n' => {
                out.push(' ');
            }
            // Tabs: convert to single space
            '\t' => {
                out.push(' ');
            }
            // BiDi formatting controls and zero-width spaces: omit to prevent reverse spoofing
            c if is_bidi_or_formatting_control(c) => {}
            // C0/C1 control characters: escape or replace
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, r"\u{{{:04x}}}", c as u32);
            }
            // Standard printable character
            c => {
                out.push(c);
            }
        }
    }

    out
}

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
        // Skip zero-width control / bidi graphemes so they are not copied (H-23)
        if g.chars()
            .all(|c| c.is_control() || is_bidi_or_formatting_control(c))
        {
            continue;
        }
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
/// using the standard `"..."` ellipsis, with terminal-safe escaping applied (H-23).
pub fn truncate_str(text: &str, max_width: usize) -> String {
    let safe_text = escape_terminal_safe(text);
    truncate_to_width(&safe_text, max_width, DEFAULT_ELLIPSIS)
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
