use flowguard::presentation::truncate::{truncate_str, truncate_to_graphemes, truncate_to_width};
use unicode_width::UnicodeWidthStr;

// 1. Ordinary ASCII text
#[test]
fn test_01_ordinary_ascii() {
    let text = "Hello, FlowGuard focus assistant!";
    let truncated = truncate_str(text, 20);
    assert_eq!(truncated, "Hello, FlowGuard ...");
    assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 20);
}

// 2. Mathematical Unicode characters (e.g. 𝒍)
#[test]
fn test_02_mathematical_unicode_script() {
    // '𝒍' is U+1D4C1 (Mathematical Script Small L), which is 4 bytes: F0 9D 93 81
    let text = "Function 𝒍(x) minimizes distraction loss";
    let truncated = truncate_str(text, 25);
    assert!(truncated.ends_with("..."));
    assert!(UnicodeWidthStr::width(truncated.as_str()) <= 25);
    // Must contain valid UTF-8 and no panic
    assert!(truncated.starts_with("Function 𝒍(x)"));
}

// 3. Emoji (standard, wide, and multi-codepoint ZWJ sequences)
#[test]
fn test_03_emoji_handling() {
    // Single emoji (crab is width 2)
    let crab = "🦀 Crab Focus Mode";
    let truncated_crab = truncate_str(crab, 12);
    assert!(UnicodeWidthStr::width(truncated_crab.as_str()) <= 12);
    assert!(truncated_crab.ends_with("..."));

    // Multi-codepoint ZWJ family emoji (👨‍👩‍👧‍👦 is 1 grapheme cluster, width 2)
    let family = "👨‍👩‍👧‍👦 Family photo in browser tab";
    let truncated_family = truncate_str(family, 15);
    assert!(UnicodeWidthStr::width(truncated_family.as_str()) <= 15);
    assert!(truncated_family.ends_with("..."));
    // The entire family emoji should either be included or excluded, never split
    assert!(truncated_family.starts_with("👨‍👩‍👧‍👦") || truncated_family == "...");

    // Flag emoji (2 regional indicator symbols, width 2)
    let flag = "🇺🇸 United States Government Portal";
    let truncated_flag = truncate_str(flag, 14);
    assert!(UnicodeWidthStr::width(truncated_flag.as_str()) <= 14);
    assert!(truncated_flag.starts_with("🇺🇸"));
}

// 4. Accented characters
#[test]
fn test_04_accented_characters() {
    let french = "Café au lait et crème brûlée dans le restaurant";
    let truncated = truncate_str(french, 25);
    assert!(UnicodeWidthStr::width(truncated.as_str()) <= 25);
    assert!(truncated.ends_with("..."));
    assert!(truncated.starts_with("Café au lait"));
}

// 5. Non-English text (Japanese CJK, Hindi Devanagari, Arabic RTL, Cyrillic)
#[test]
fn test_05_non_english_scripts() {
    // Japanese: each fullwidth kanji/kana is 2 visual columns
    let japanese = "フローガード — 集中アシスタント";
    let truncated_ja = truncate_str(japanese, 15);
    assert!(UnicodeWidthStr::width(truncated_ja.as_str()) <= 15);
    assert!(truncated_ja.ends_with("..."));

    // Hindi: complex conjuncts
    let hindi = "फ्लो गार्ड - उत्पादकता सहायक";
    let truncated_hi = truncate_str(hindi, 16);
    assert!(UnicodeWidthStr::width(truncated_hi.as_str()) <= 16);
    assert!(truncated_hi.ends_with("..."));

    // Arabic RTL
    let arabic = "حارس التدفق - مساعد التركيز الفعال";
    let truncated_ar = truncate_str(arabic, 20);
    assert!(UnicodeWidthStr::width(truncated_ar.as_str()) <= 20);
    assert!(truncated_ar.ends_with("..."));

    // Cyrillic
    let cyrillic = "FlowGuard — Фокус и продуктивность разработчика";
    let truncated_ru = truncate_str(cyrillic, 25);
    assert!(UnicodeWidthStr::width(truncated_ru.as_str()) <= 25);
    assert!(truncated_ru.ends_with("..."));
}

// 6. Combining characters (decomposed accents)
#[test]
fn test_06_combining_characters() {
    // 'e' followed by combining acute accent U+0301 (2 codepoints, 1 grapheme cluster, width 1)
    let text = "e\u{0301}l\u{0301}e\u{0301}p\u{0301}h\u{0301}a\u{0301}n\u{0301}t is a large mammal";
    let truncated = truncate_str(text, 15);
    assert!(UnicodeWidthStr::width(truncated.as_str()) <= 15);
    assert!(truncated.ends_with("..."));
    // Combining accent must never be separated from its base character
    assert!(truncated.starts_with("e\u{0301}l\u{0301}e\u{0301}"));
}

// 7. Zero-length input
#[test]
fn test_07_zero_length_input() {
    assert_eq!(truncate_str("", 10), "");
    assert_eq!(truncate_str("", 0), "");
    assert_eq!(truncate_to_width("", 5, "..."), "");
    assert_eq!(truncate_to_graphemes("", 5, "..."), "");
}

// 8. Text shorter than the truncation limit
#[test]
fn test_08_text_shorter_than_limit() {
    let text = "Short";
    assert_eq!(truncate_str(text, 10), "Short");
    assert_eq!(truncate_str("Short", 5), "Short");
}

// 9. Text exactly at the truncation limit
#[test]
fn test_09_text_exactly_at_limit() {
    let text = "1234567890";
    assert_eq!(truncate_str(text, 10), "1234567890");

    let ja = "あいう"; // 3 CJK chars * 2 width = 6 columns
    assert_eq!(truncate_str(ja, 6), "あいう");
}

// 10. Text longer than the truncation limit
#[test]
fn test_10_text_longer_than_limit() {
    let text = "12345678901"; // 11 chars
    let truncated = truncate_str(text, 10);
    assert_eq!(truncated, "1234567...");
    assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 10);
}

// 11. Unicode characters at the exact truncation boundary
#[test]
fn test_11_unicode_at_exact_truncation_boundary() {
    // 5 ASCII chars (width 5) + 1 CJK char (width 2) = total width 7
    // If limit is 6, available content width with ellipsis ("...") is 3
    // "ABCDE" -> first 3 fit ("ABC..."), width 6
    let text = "ABCDE日本語";
    let truncated = truncate_to_width(text, 8, "...");
    assert_eq!(truncated, "ABCDE...");
    assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 8);

    // Testing when wide character sits right at available boundary:
    // Limit = 6, ellipsis = "..." (width 3), available content width = 3
    // text = "A" (width 1) + "日" (width 2) = width 3 (fits exactly!)
    // "本" (width 2) would exceed available content width (3 + 2 = 5 > 3)
    let text2 = "A日本語";
    let truncated2 = truncate_to_width(text2, 6, "...");
    assert_eq!(truncated2, "A日...");
    assert_eq!(UnicodeWidthStr::width(truncated2.as_str()), 6);
}

// 12. Small limits smaller than ellipsis
#[test]
fn test_12_very_small_limits() {
    let text = "Hello, world!";
    assert_eq!(truncate_to_width(text, 0, "..."), "");
    assert_eq!(truncate_to_width(text, 1, "..."), ".");
    assert_eq!(truncate_to_width(text, 2, "..."), "..");
    assert_eq!(truncate_to_width(text, 3, "..."), "...");
    assert_eq!(truncate_to_width(text, 4, "..."), "H...");
}

// 13. EXACT REGRESSION TEST: The real-world crash reproduced and verified
#[test]
fn test_13_exact_regression_reproduction() {
    // In the reported crash:
    // "end byte index 37 is not a char boundary; it is inside '𝒍' (bytes 34..38 of string)"
    // Construct the exact byte scenario: 34 ASCII bytes, followed by '𝒍' (4 bytes: bytes 34..38).
    let prefix_34_bytes = "A".repeat(34);
    assert_eq!(prefix_34_bytes.len(), 34);

    let script_l = '𝒍'; // '\u{1D4C1}'
    assert_eq!(script_l.len_utf8(), 4);

    let suffix = " - Advanced Machine Learning Tutorial";
    let full_window_title = format!("{}{}{}", prefix_34_bytes, script_l, suffix);

    // Verify byte layout matches the crash report:
    // Byte 34 is the start of '𝒍' (4 bytes: 34..38)
    assert_eq!(
        &full_window_title.as_bytes()[34..38],
        &[0xF0, 0x9D, 0x92, 0x8D]
    );
    // Byte index 37 is inside '𝒍'!

    // The old code did `&t[..37]`, which panicked:
    let panic_result = std::panic::catch_unwind(|| {
        let _ = &full_window_title[..37];
    });
    assert!(
        panic_result.is_err(),
        "Old slice logic MUST panic on byte index 37"
    );

    // The new Unicode-safe truncation MUST NOT panic and must produce a valid string
    let safe_result = truncate_str(&full_window_title, 40);
    assert!(safe_result.ends_with("..."));
    assert!(UnicodeWidthStr::width(safe_result.as_str()) <= 40);

    // Also test truncation at exact length 38 (script_l fits)
    let safe_result_38 = truncate_to_width(&full_window_title, 38, "...");
    assert!(UnicodeWidthStr::width(safe_result_38.as_str()) <= 38);

    // Also test snapshot limit (60 columns)
    let snapshot_result = truncate_str(&full_window_title, 60);
    assert!(UnicodeWidthStr::width(snapshot_result.as_str()) <= 60);
}

// 14. Grapheme cluster count truncation
#[test]
fn test_14_truncate_to_graphemes() {
    let text = "👨‍👩‍👧‍👦 is family, 🦀 is crab";
    let truncated = truncate_to_graphemes(text, 10, "...");
    assert!(truncated.ends_with("..."));
    assert_eq!(
        unicode_segmentation::UnicodeSegmentation::graphemes(truncated.as_str(), true).count(),
        10
    );
}

// 15. Presentation layer end-to-end smoke test with extreme Unicode titles
#[test]
fn test_15_presentation_layer_unicode_smoke_test() {
    use chrono::Utc;
    use flowguard::domain::{
        ActivityCapabilities, ActivityObservation, BrowserContext, FreshnessState, IdleState,
        SourceHealth,
    };
    use flowguard::presentation::console::{print_observation_change, print_observation_snapshot};

    let titles = [
        format!("{}𝒍 - Mathematical Script Small L", "A".repeat(34)),
        "🦀 Rust - 👨‍👩‍👧‍👦 Family Tree Application 🚀".to_string(),
        "フローガード — 集中アシスタント / Japanese Window Title".to_string(),
        "फ्लो गार्ड - उत्पादकता सहायक / Hindi Window Title".to_string(),
        "e\u{0301}l\u{0301}e\u{0301}p\u{0301}h\u{0301}a\u{0301}n\u{0301}t with combining accents"
            .to_string(),
        "".to_string(),
        "Exact 40 char title: 12345678901234567890".to_string(),
    ];

    for title in &titles {
        let obs = ActivityObservation {
            observed_at: Utc::now(),
            application: Some("chrome.exe".to_string()),
            window_title: Some(title.clone()),
            browser: Some(BrowserContext {
                url: Some("https://example.com/search?q=test&lang=ja".to_string()),
                title: Some(title.clone()),
                audible: Some(false),
                incognito: Some(false),
            }),
            idle_state: IdleState::Active,
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: true,
                incognito_detection: true,
            },
            source_health: SourceHealth::Connected,
            window_freshness: FreshnessState::Fresh,
            idle_freshness: FreshnessState::Fresh,
            browser_freshness: FreshnessState::Fresh,
            diagnostics: vec![],
        };

        // Must never panic in compact or verbose mode
        print_observation_change(&obs, false);
        print_observation_change(&obs, true);
        print_observation_snapshot(&obs, false);
        print_observation_snapshot(&obs, true);
    }
}
