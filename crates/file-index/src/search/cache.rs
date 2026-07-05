use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::full_text::TantivySearchRuntime;
use super::ignore_policy::exclude_rules_hash;
use super::manifest::SearchCatalogIdentity;
use super::path_encoding::path_storage_key;
use super::store;
use super::types::DirectoryErrorPolicy;
use crate::profile::MediaMetadataScope;
use crate::IndexError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct QueryRuntimeCacheKey {
    index_dir: PathBuf,
    root_key: String,
    include_hidden: bool,
    exclude_rules_hash: String,
    directory_error_policy: DirectoryErrorPolicy,
    media_metadata_scope: MediaMetadataScope,
}

pub(crate) struct SearchQueryRuntime {
    index_dir: PathBuf,
    root: PathBuf,
    identity: SearchCatalogIdentity,
    full_text_runtime: Mutex<Option<Option<Arc<TantivySearchRuntime>>>>,
}

static LOADED_QUERY_RUNTIMES: OnceLock<
    Mutex<HashMap<QueryRuntimeCacheKey, Arc<SearchQueryRuntime>>>,
> = OnceLock::new();

pub(crate) fn query_runtime_for_index(
    index_dir: &Path,
    root: &Path,
    include_hidden: bool,
    exclude_patterns: &[String],
    directory_error_policy: DirectoryErrorPolicy,
    media_metadata_scope: MediaMetadataScope,
) -> Result<Arc<SearchQueryRuntime>, IndexError> {
    let manifest = store::read_manifest(index_dir)?;
    manifest.validate_for(
        index_dir,
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        media_metadata_scope,
    )?;
    let key = QueryRuntimeCacheKey::new(
        index_dir,
        root,
        include_hidden,
        exclude_patterns,
        directory_error_policy,
        media_metadata_scope,
    );
    if let Some(runtime) = cached_runtime(&key, &manifest.identity()) {
        return Ok(runtime);
    }

    let runtime = Arc::new(SearchQueryRuntime::new(
        index_dir,
        root,
        manifest.identity(),
    ));
    cache_runtime(key, Arc::clone(&runtime));
    Ok(runtime)
}

pub(crate) fn clear_query_cache() {
    if let Ok(mut runtimes) = loaded_query_runtimes().lock() {
        runtimes.clear();
    }
}

fn cached_runtime(
    key: &QueryRuntimeCacheKey,
    expected_identity: &SearchCatalogIdentity,
) -> Option<Arc<SearchQueryRuntime>> {
    let mut runtimes = loaded_query_runtimes().lock().ok()?;
    let runtime = runtimes.get(key)?.clone();
    if runtime.matches_identity(expected_identity) {
        Some(runtime)
    } else {
        runtimes.remove(key);
        None
    }
}

fn cache_runtime(key: QueryRuntimeCacheKey, runtime: Arc<SearchQueryRuntime>) {
    if let Ok(mut runtimes) = loaded_query_runtimes().lock() {
        runtimes.insert(key, runtime);
    }
}

fn loaded_query_runtimes() -> &'static Mutex<HashMap<QueryRuntimeCacheKey, Arc<SearchQueryRuntime>>>
{
    LOADED_QUERY_RUNTIMES.get_or_init(|| Mutex::new(HashMap::new()))
}

impl SearchQueryRuntime {
    fn new(index_dir: &Path, root: &Path, identity: SearchCatalogIdentity) -> Self {
        Self {
            index_dir: index_dir.to_path_buf(),
            root: root.to_path_buf(),
            identity,
            full_text_runtime: Mutex::new(None),
        }
    }

    pub(crate) fn index_dir(&self) -> &Path {
        &self.index_dir
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn full_text_runtime(
        &self,
    ) -> Result<Option<Arc<TantivySearchRuntime>>, IndexError> {
        let cached_runtime = self
            .full_text_runtime
            .lock()
            .map_err(|_| IndexError::store(&self.index_dir, "search query runtime cache poisoned"))?
            .clone();
        if let Some(runtime) = cached_runtime {
            return Ok(runtime);
        }

        let runtime = TantivySearchRuntime::open(&self.index_dir)?.map(Arc::new);
        let mut cached_runtime = self.full_text_runtime.lock().map_err(|_| {
            IndexError::store(&self.index_dir, "search query runtime cache poisoned")
        })?;
        if let Some(runtime) = cached_runtime.clone() {
            return Ok(runtime);
        }
        *cached_runtime = Some(runtime.clone());
        Ok(runtime)
    }

    fn matches_identity(&self, expected_identity: &SearchCatalogIdentity) -> bool {
        &self.identity == expected_identity
    }
}

impl QueryRuntimeCacheKey {
    fn new(
        index_dir: &Path,
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
        directory_error_policy: DirectoryErrorPolicy,
        media_metadata_scope: MediaMetadataScope,
    ) -> Self {
        Self {
            index_dir: index_dir.to_path_buf(),
            root_key: path_storage_key(root),
            include_hidden,
            exclude_rules_hash: exclude_rules_hash(exclude_patterns),
            directory_error_policy,
            media_metadata_scope,
        }
    }
}
