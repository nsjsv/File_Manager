use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::catalog::SearchCatalog;
use super::path_encoding::path_storage_key;
use super::store::{self, exclude_rules_hash, SearchCatalogIdentity, SearchIndexManifest};
use crate::FileError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CatalogCacheKey {
    index_dir: PathBuf,
    root_key: String,
    include_hidden: bool,
    exclude_rules_hash: String,
}

static LOADED_CATALOGS: OnceLock<Mutex<HashMap<CatalogCacheKey, Arc<SearchCatalog>>>> =
    OnceLock::new();

pub(crate) fn catalog_for_index(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
) -> Result<Arc<SearchCatalog>, FileError> {
    let manifest = store::read_manifest(index_dir)?;
    manifest.validate_for(index_dir, root, include_hidden, exclude_patterns)?;
    let key = CatalogCacheKey::new(index_dir, root, include_hidden, exclude_patterns);
    if let Some(catalog) = cached_catalog(&key, &manifest.identity()) {
        return Ok(catalog);
    }

    let (manifest, records) =
        store::load_catalog(index_dir, root, include_hidden, exclude_patterns)?;
    let catalog = Arc::new(SearchCatalog::from_records(
        root.to_path_buf(),
        records,
        Some(&manifest),
    ));
    cache_catalog(key, Arc::clone(&catalog));
    Ok(catalog)
}

pub(crate) fn cache_built_catalog(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
    manifest: &SearchIndexManifest,
    catalog: SearchCatalog,
) {
    let key = CatalogCacheKey::new(index_dir, root, include_hidden, exclude_patterns);
    let catalog = Arc::new(catalog);
    if catalog.identity() == Some(&manifest.identity()) {
        cache_catalog(key, catalog);
    }
}

fn cached_catalog(
    key: &CatalogCacheKey,
    expected_identity: &SearchCatalogIdentity,
) -> Option<Arc<SearchCatalog>> {
    let catalogs = loaded_catalogs().lock().ok()?;
    let catalog = catalogs.get(key)?;
    if catalog.identity() == Some(expected_identity) {
        Some(Arc::clone(catalog))
    } else {
        None
    }
}

fn cache_catalog(key: CatalogCacheKey, catalog: Arc<SearchCatalog>) {
    if let Ok(mut catalogs) = loaded_catalogs().lock() {
        catalogs.insert(key, catalog);
    }
}

fn loaded_catalogs() -> &'static Mutex<HashMap<CatalogCacheKey, Arc<SearchCatalog>>> {
    LOADED_CATALOGS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl CatalogCacheKey {
    fn new(
        index_dir: &Path,
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
    ) -> Self {
        Self {
            index_dir: index_dir.to_path_buf(),
            root_key: path_storage_key(root),
            include_hidden,
            exclude_rules_hash: exclude_rules_hash(exclude_patterns),
        }
    }
}
