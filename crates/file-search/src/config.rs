use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) const MAX_CONFIGURED_ROOTS: usize = 32;
pub(crate) const MAX_CONFIGURED_EXCLUDED_PATHS: usize = 256;
pub(crate) const MAX_CONFIGURED_PATH_BYTES: usize = 4_096;
pub(crate) const MAX_CONFIGURED_TOTAL_PATH_BYTES: usize = 262_144;
pub(crate) const MAX_EXTRACT_BYTES: u64 = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIndexConfig {
    pub roots: Vec<PathBuf>,
    pub excluded_paths: Vec<PathBuf>,
    pub content_indexing_enabled: bool,
    pub max_extract_bytes: u64,
}

impl Default for SearchIndexConfig {
    fn default() -> Self {
        let roots = dirs::home_dir().into_iter().collect();
        Self {
            roots,
            excluded_paths: Vec::new(),
            content_indexing_enabled: true,
            max_extract_bytes: 2_000_000,
        }
    }
}

impl SearchIndexConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.roots.len() > MAX_CONFIGURED_ROOTS {
            return Err(format!(
                "configured search roots exceed the {MAX_CONFIGURED_ROOTS} entry limit"
            ));
        }
        if self.excluded_paths.len() > MAX_CONFIGURED_EXCLUDED_PATHS {
            return Err(format!(
                "configured search exclusions exceed the {MAX_CONFIGURED_EXCLUDED_PATHS} entry limit"
            ));
        }
        if self.max_extract_bytes > MAX_EXTRACT_BYTES {
            return Err(format!(
                "configured extraction limit {} exceeds the {MAX_EXTRACT_BYTES} byte service budget",
                self.max_extract_bytes
            ));
        }

        let mut total_path_bytes = 0_usize;
        for path in self.roots.iter().chain(&self.excluded_paths) {
            let path_bytes = path.as_os_str().as_encoded_bytes().len();
            if path_bytes > MAX_CONFIGURED_PATH_BYTES {
                return Err(format!(
                    "configured search path exceeds the {MAX_CONFIGURED_PATH_BYTES} byte limit: {}",
                    path.display()
                ));
            }
            total_path_bytes = total_path_bytes.saturating_add(path_bytes);
            if total_path_bytes > MAX_CONFIGURED_TOTAL_PATH_BYTES {
                return Err(format!(
                    "configured search paths exceed the {MAX_CONFIGURED_TOTAL_PATH_BYTES} byte total limit"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SearchExcludeRules {
    excluded_paths: Vec<PathBuf>,
}

impl SearchExcludeRules {
    pub fn new(excluded_paths: Vec<PathBuf>) -> Self {
        Self { excluded_paths }
    }

    pub fn should_skip_directory(&self, path: &Path) -> bool {
        self.should_skip_path(path)
            || is_hidden_name(path.file_name())
            || has_ignored_directory_name(path.file_name())
            || path.join(".nomedia").is_file()
    }

    pub fn should_skip_path(&self, path: &Path) -> bool {
        self.excluded_paths
            .iter()
            .any(|excluded| path == excluded || path.starts_with(excluded))
    }
}

fn is_hidden_name(name: Option<&OsStr>) -> bool {
    name.and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with('.') && name != "." && name != "..")
}

fn has_ignored_directory_name(name: Option<&OsStr>) -> bool {
    matches!(
        name.and_then(OsStr::to_str),
        Some(".git" | "node_modules" | "target" | ".cache")
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        SearchExcludeRules, SearchIndexConfig, MAX_CONFIGURED_EXCLUDED_PATHS,
        MAX_CONFIGURED_PATH_BYTES, MAX_CONFIGURED_ROOTS, MAX_EXTRACT_BYTES,
    };

    #[test]
    fn default_content_payload_limit_is_two_decimal_megabytes() {
        assert_eq!(SearchIndexConfig::default().max_extract_bytes, 2_000_000);
    }

    #[test]
    fn skips_hidden_and_configured_directories() {
        let rules = SearchExcludeRules::new(vec![Path::new("/home/me/private").to_path_buf()]);

        assert!(rules.should_skip_directory(Path::new("/home/me/.hidden")));
        assert!(rules.should_skip_directory(Path::new("/home/me/project/.git")));
        assert!(rules.should_skip_directory(Path::new("/home/me/private")));
        assert!(!rules.should_skip_directory(Path::new("/home/me/Documents")));
    }

    #[test]
    fn configured_exclusion_applies_to_files_without_content_size_policy() {
        let rules = SearchExcludeRules::new(vec![Path::new("/home/me/private").to_path_buf()]);

        assert!(rules.should_skip_path(Path::new("/home/me/private/big.txt")));
        assert!(!rules.should_skip_path(Path::new("/home/me/public/big.txt")));
    }

    #[test]
    fn service_configuration_accepts_values_within_resource_boundaries() {
        SearchIndexConfig::default().validate().unwrap();
        SearchIndexConfig {
            roots: Vec::new(),
            excluded_paths: Vec::new(),
            content_indexing_enabled: false,
            max_extract_bytes: 0,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn service_configuration_rejects_unbounded_collections_and_payloads() {
        let too_many_roots = SearchIndexConfig {
            roots: vec![PathBuf::from("/tmp"); MAX_CONFIGURED_ROOTS + 1],
            ..SearchIndexConfig::default()
        };
        assert!(too_many_roots.validate().is_err());

        let too_many_exclusions = SearchIndexConfig {
            excluded_paths: vec![PathBuf::from("/tmp"); MAX_CONFIGURED_EXCLUDED_PATHS + 1],
            ..SearchIndexConfig::default()
        };
        assert!(too_many_exclusions.validate().is_err());

        let oversized_path = SearchIndexConfig {
            roots: vec![PathBuf::from("x".repeat(MAX_CONFIGURED_PATH_BYTES + 1))],
            ..SearchIndexConfig::default()
        };
        assert!(oversized_path.validate().is_err());

        let oversized_extraction = SearchIndexConfig {
            max_extract_bytes: MAX_EXTRACT_BYTES + 1,
            ..SearchIndexConfig::default()
        };
        assert!(oversized_extraction.validate().is_err());
    }
}
