const DEFAULT_EXCLUDED_DIRECTORY_PATTERNS: &[&str] = &[
    ".cargo/",
    ".npm/",
    ".pnpm/",
    ".pnpm-store/",
    "node_modules/",
    "target/",
];

pub fn default_search_index_exclude_patterns() -> &'static [&'static str] {
    DEFAULT_EXCLUDED_DIRECTORY_PATTERNS
}

pub(crate) fn exclude_rules_hash(patterns: &[String]) -> String {
    let mut normalized = patterns
        .iter()
        .map(|pattern| pattern.trim().to_owned())
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();

    let mut hasher = blake3::Hasher::new();
    for pattern in normalized {
        hasher.update(pattern.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_rules_hash_is_order_independent() {
        let left = vec!["target/".to_owned(), "node_modules/".to_owned()];
        let right = vec!["node_modules/".to_owned(), "target/".to_owned()];

        assert_eq!(exclude_rules_hash(&left), exclude_rules_hash(&right));
    }

    #[test]
    fn exclude_rules_hash_changes_when_default_pattern_is_removed() {
        let with_target = vec!["node_modules/".to_owned(), "target/".to_owned()];
        let without_target = vec!["node_modules/".to_owned()];

        assert_ne!(
            exclude_rules_hash(&with_target),
            exclude_rules_hash(&without_target)
        );
    }
}
