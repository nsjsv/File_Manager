use super::*;

#[test]
fn detects_chinese_from_lang_prefix() {
    let language = detect_system_language_with(|key| match key {
        "LANG" => Some("zh_CN.UTF-8".to_owned()),
        _ => None,
    });
    assert_eq!(language, UiLanguage::Chinese);
}

#[test]
fn detects_chinese_from_language_fallback() {
    let language = detect_system_language_with(|key| match key {
        "LANGUAGE" => Some("en_US:zh_CN".to_owned()),
        _ => None,
    });
    assert_eq!(language, UiLanguage::Chinese);
}

#[test]
fn defaults_to_english_for_non_chinese_locale() {
    let language = detect_system_language_with(|key| match key {
        "LANG" => Some("en_US.UTF-8".to_owned()),
        _ => None,
    });
    assert_eq!(language, UiLanguage::English);
}

#[test]
fn translates_known_static_text() {
    for text in [
        "Search",
        "Any type",
        "Any content",
        "Spreadsheets",
        "More",
        "Yesterday",
        "Past year",
        "Name & content",
        "Name only",
        "Reset filters",
        "Content indexing is unavailable; matching file names only.",
        "Past 30 days",
        "Close search",
        "Showing the first 100 results",
        "Logs",
        "Appearance",
        "Files",
        "Startup location",
        "Service and Index",
        "Index File Contents",
        "Discrete GPU",
        "Audio preview",
        "Text preview is not ready",
        "No pending conflicts",
        "File",
        "Symbolic Link",
        "No compression",
        "Open Terminal Here",
    ] {
        assert_ne!(translate(UiLanguage::Chinese, text), text);
    }
}

#[test]
fn translates_known_dynamic_text() {
    for (text, expected) in [
        (
            "Progress: 12,345 items scanned",
            "进度：已扫描 12,345 个项目",
        ),
        ("Indexed: 98,765 items", "已索引：98,765 个项目"),
        (
            "Index maintenance: Degraded (watch gap)",
            "索引维护：已降级（watch gap）",
        ),
        ("Search in /home/test", "搜索范围：/home/test"),
        (
            "Partial results after inspecting 50000 entries",
            "部分结果，已检查 50000 个条目",
        ),
        (
            "Search endpoint unavailable: socket closed",
            "搜索端点不可用：socket closed",
        ),
        (
            "Maximum content extraction: 8 MiB",
            "最大内容提取大小：8 MiB",
        ),
        ("Volume 75%", "音量 75%"),
    ] {
        assert_eq!(translate(UiLanguage::Chinese, text), expected);
    }
}
