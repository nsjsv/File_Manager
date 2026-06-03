use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs as std_fs, thread};

use ignore::WalkBuilder;
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};
use tantivy::collector::{DocSetCollector, TopDocs};
use tantivy::query::{AllQuery, QueryParser};
use tantivy::schema::{Field, Schema, TantivyDocument, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter};
use tokio::fs;

use crate::scan::{entry_from_dir_entry, is_hidden_name};
use crate::{DirectoryEntry, FileError, FileKind, ScanWarning};

const DEFAULT_SEARCH_LIMIT: usize = 50;
const TANTIVY_WRITER_MEMORY_BYTES: usize = 15_000_000;
const TANTIVY_BOOST_LIMIT: usize = 512;
const TANTIVY_TOP_BONUS: u32 = 2_000;
const NUCLEO_NAME_BONUS: u32 = 1_000;
const INDEX_THROTTLE_EVERY: usize = 128;
const INDEX_THROTTLE_SLEEP: Duration = Duration::from_millis(2);
const INDEX_META_FILE: &str = "meta.json";
const SEARCH_FIELD_NAME: &str = "name";
const SEARCH_FIELD_PATH: &str = "path";
const SEARCH_FIELD_PATH_KEY: &str = "path_key";
const SEARCH_FIELD_KIND: &str = "kind";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOptions {
    pub include_hidden: bool,
    pub limit: usize,
}

impl Default for FileSearchOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            limit: DEFAULT_SEARCH_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchOutcome {
    pub root: PathBuf,
    pub matches: Vec<FileSearchMatch>,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexOptions {
    pub include_hidden: bool,
}

impl Default for FileSearchIndexOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchIndexOutcome {
    pub root: PathBuf,
    pub index_dir: PathBuf,
    pub indexed_count: usize,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchMatch {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub name: OsString,
    pub kind: FileKind,
    pub rank_score: u32,
}

impl FileSearchMatch {
    pub fn name(&self) -> &OsStr {
        &self.name
    }
}

pub async fn search_file_tree(
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(FileSearchOutcome {
            root,
            matches: Vec::new(),
            skipped: Vec::new(),
        });
    }

    let (candidates, skipped) = collect_search_candidates(&root, &options).await?;
    let candidates = Arc::new(candidates);
    let boost_root = root.clone();
    let boost_query = query.clone();
    let boost_candidates = Arc::clone(&candidates);
    let tantivy_boosts = tokio::task::spawn_blocking(move || {
        tantivy_boosts_for_query(&boost_root, &boost_query, boost_candidates.as_slice())
    })
    .await
    .map_err(|error| search_index_error(&root, error))??;
    let matches = nucleo_ranked_matches(
        &query,
        candidates.as_slice(),
        &tantivy_boosts,
        options.limit.max(1),
    );

    Ok(FileSearchOutcome {
        root,
        matches,
        skipped,
    })
}

pub async fn build_file_search_index(
    root: impl AsRef<Path>,
    index_dir: impl AsRef<Path>,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        build_file_search_index_blocking(&root, &index_dir, options)
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub async fn search_file_index(
    index_dir: impl AsRef<Path>,
    root: impl AsRef<Path>,
    query: impl AsRef<str>,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, FileError> {
    let root = root.as_ref().to_path_buf();
    let index_dir = index_dir.as_ref().to_path_buf();
    let query = query.as_ref().trim().to_owned();
    if query.is_empty() {
        return Ok(FileSearchOutcome {
            root,
            matches: Vec::new(),
            skipped: Vec::new(),
        });
    }
    let join_root = root.clone();

    tokio::task::spawn_blocking(move || {
        search_file_index_blocking(&index_dir, &root, &query, options)
    })
    .await
    .map_err(|error| search_index_error(&join_root, error))?
}

pub fn file_search_index_exists(index_dir: impl AsRef<Path>) -> bool {
    index_dir.as_ref().join(INDEX_META_FILE).is_file()
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    path: PathBuf,
    relative_path: PathBuf,
    name: OsString,
    kind: FileKind,
    name_text: String,
    path_text: String,
    storage_key: String,
}

#[derive(Clone, Copy)]
struct FileSearchFields {
    name: Field,
    path: Field,
    path_key: Field,
    kind: Field,
}

impl FileSearchFields {
    fn from_schema(root: &Path, schema: &Schema) -> Result<Self, FileError> {
        Ok(Self {
            name: schema
                .get_field(SEARCH_FIELD_NAME)
                .map_err(|error| search_index_error(root, error))?,
            path: schema
                .get_field(SEARCH_FIELD_PATH)
                .map_err(|error| search_index_error(root, error))?,
            path_key: schema
                .get_field(SEARCH_FIELD_PATH_KEY)
                .map_err(|error| search_index_error(root, error))?,
            kind: schema
                .get_field(SEARCH_FIELD_KIND)
                .map_err(|error| search_index_error(root, error))?,
        })
    }
}

async fn collect_search_candidates(
    root: &Path,
    options: &FileSearchOptions,
) -> Result<(Vec<SearchCandidate>, Vec<ScanWarning>), FileError> {
    let mut directories = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    let mut skipped = Vec::new();

    while let Some(directory) = directories.pop() {
        let mut reader = match fs::read_dir(&directory).await {
            Ok(reader) => reader,
            Err(source) if directory == root => {
                return Err(FileError::ReadDirectory {
                    path: directory,
                    source,
                })
            }
            Err(source) => {
                skipped.push(ScanWarning {
                    path: directory,
                    message: source.to_string(),
                });
                continue;
            }
        };

        loop {
            let dir_entry = match reader.next_entry().await {
                Ok(Some(dir_entry)) => dir_entry,
                Ok(None) => break,
                Err(source) => {
                    skipped.push(ScanWarning {
                        path: directory.clone(),
                        message: source.to_string(),
                    });
                    break;
                }
            };

            let name = dir_entry.file_name();
            let is_hidden = is_hidden_name(&name);
            if is_hidden && !options.include_hidden {
                continue;
            }

            match entry_from_dir_entry(dir_entry, name, is_hidden).await {
                Ok(entry) => {
                    if entry.kind == FileKind::Directory {
                        directories.push(entry.path.clone());
                    }
                    candidates.push(search_candidate(root, entry));
                }
                Err(FileError::Metadata { path, source }) => skipped.push(ScanWarning {
                    path,
                    message: source.to_string(),
                }),
                Err(error) => return Err(error),
            }
        }
    }

    Ok((candidates, skipped))
}

fn search_candidate(root: &Path, entry: DirectoryEntry) -> SearchCandidate {
    let relative_path = entry
        .path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| entry.path.clone());
    let name_text = entry.name().to_string_lossy().into_owned();
    let path_text = relative_path.to_string_lossy().into_owned();

    SearchCandidate {
        storage_key: path_storage_key(&entry.path),
        path: entry.path,
        relative_path,
        name: entry.name,
        kind: entry.kind,
        name_text,
        path_text,
    }
}

fn search_candidate_from_path(root: &Path, path: PathBuf, kind: FileKind) -> SearchCandidate {
    let relative_path = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.clone());
    let name = path
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_else(|| path.as_os_str().to_os_string());
    let name_text = name.to_string_lossy().into_owned();
    let path_text = relative_path.to_string_lossy().into_owned();
    let storage_key = path_storage_key(&path);

    SearchCandidate {
        path,
        relative_path,
        name,
        kind,
        name_text,
        path_text,
        storage_key,
    }
}

fn tantivy_boosts_for_query(
    root: &Path,
    query: &str,
    candidates: &[SearchCandidate],
) -> Result<HashMap<String, u32>, FileError> {
    let (index, fields) = build_tantivy_index(root, candidates)?;
    tantivy_boosts_for_index(root, &index, fields, query)
}

fn tantivy_boosts_for_index(
    root: &Path,
    index: &Index,
    fields: FileSearchFields,
    query: &str,
) -> Result<HashMap<String, u32>, FileError> {
    let reader = index
        .reader()
        .map_err(|error| search_index_error(root, error))?;
    let searcher = reader.searcher();
    let query_parser = QueryParser::for_index(&index, vec![fields.name, fields.path]);
    let (tantivy_query, _) = query_parser.parse_query_lenient(query);
    let top_docs = searcher
        .search(
            &tantivy_query,
            &TopDocs::with_limit(TANTIVY_BOOST_LIMIT).order_by_score(),
        )
        .map_err(|error| search_index_error(root, error))?;

    let mut boosts = HashMap::with_capacity(top_docs.len());
    for (rank, (_score, doc_address)) in top_docs.into_iter().enumerate() {
        let document = searcher
            .doc::<TantivyDocument>(doc_address)
            .map_err(|error| search_index_error(root, error))?;
        let Some(path_key) = document
            .get_first(fields.path_key)
            .and_then(|value| value.as_str())
        else {
            continue;
        };

        let bonus = TANTIVY_TOP_BONUS.saturating_sub(rank as u32);
        boosts.insert(path_key.to_owned(), bonus);
    }

    Ok(boosts)
}

fn build_tantivy_index(
    root: &Path,
    candidates: &[SearchCandidate],
) -> Result<(Index, FileSearchFields), FileError> {
    let (schema, fields) = file_search_schema();
    let index = Index::create_in_ram(schema);
    let mut index_writer: IndexWriter = index
        .writer(TANTIVY_WRITER_MEMORY_BYTES)
        .map_err(|error| search_index_error(root, error))?;

    for candidate in candidates {
        index_writer
            .add_document(search_document(fields, candidate))
            .map_err(|error| search_index_error(root, error))?;
    }

    index_writer
        .commit()
        .map_err(|error| search_index_error(root, error))?;

    Ok((index, fields))
}

fn nucleo_ranked_matches(
    query: &str,
    candidates: &[SearchCandidate],
    tantivy_boosts: &HashMap<String, u32>,
    limit: usize,
) -> Vec<FileSearchMatch> {
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut matches = candidates
        .iter()
        .filter_map(|candidate| {
            let rank_score = candidate_rank_score(candidate, &pattern, &mut matcher)
                .or_else(|| tantivy_boosts.get(&candidate.storage_key).copied())?;
            let rank_score = rank_score
                .saturating_add(*tantivy_boosts.get(&candidate.storage_key).unwrap_or(&0));
            Some(FileSearchMatch {
                path: candidate.path.clone(),
                relative_path: candidate.relative_path.clone(),
                name: candidate.name.clone(),
                kind: candidate.kind,
                rank_score,
            })
        })
        .collect::<Vec<_>>();

    sort_limited_search_matches(&mut matches, limit);
    matches
}

fn sort_limited_search_matches(matches: &mut Vec<FileSearchMatch>, limit: usize) {
    if matches.len() > limit {
        matches.select_nth_unstable_by(limit, compare_search_matches);
        matches.truncate(limit);
    }
    matches.sort_unstable_by(compare_search_matches);
}

fn compare_search_matches(left: &FileSearchMatch, right: &FileSearchMatch) -> Ordering {
    right
        .rank_score
        .cmp(&left.rank_score)
        .then_with(|| left.path.cmp(&right.path))
}

fn candidate_rank_score(
    candidate: &SearchCandidate,
    pattern: &Pattern,
    matcher: &mut Matcher,
) -> Option<u32> {
    let name_text = Utf32String::from(candidate.name_text.as_str());
    let path_text = Utf32String::from(candidate.path_text.as_str());
    let name_score = pattern
        .score(name_text.slice(..), matcher)
        .map(|score| score.saturating_add(NUCLEO_NAME_BONUS));
    let path_score = pattern.score(path_text.slice(..), matcher);

    name_score.max(path_score)
}

fn search_index_error(path: &Path, error: impl ToString) -> FileError {
    FileError::SearchIndex {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn build_file_search_index_blocking(
    root: &Path,
    index_dir: &Path,
    options: FileSearchIndexOptions,
) -> Result<FileSearchIndexOutcome, FileError> {
    std_fs::read_dir(root).map_err(|source| FileError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;

    let pending_index_dir = index_dir.with_extension("building");
    prepare_index_dir(root, &pending_index_dir)?;
    let (schema, fields) = file_search_schema();
    let index = Index::create_in_dir(&pending_index_dir, schema)
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut writer = index
        .writer(TANTIVY_WRITER_MEMORY_BYTES)
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut skipped = Vec::new();
    let mut indexed_count = 0;

    for result in search_walk_builder(root, index_dir, &options).build() {
        let dir_entry = match result {
            Ok(dir_entry) => dir_entry,
            Err(error) => {
                skipped.push(ignore_error_warning(root, error));
                continue;
            }
        };
        if dir_entry.depth() == 0 {
            continue;
        }

        let path = dir_entry.into_path();
        let metadata = match std_fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) => {
                skipped.push(ScanWarning {
                    path,
                    message: source.to_string(),
                });
                continue;
            }
        };
        let candidate = search_candidate_from_path(root, path, file_kind_from_metadata(&metadata));
        writer
            .add_document(search_document(fields, &candidate))
            .map_err(|error| search_index_error(index_dir, error))?;
        indexed_count += 1;

        if indexed_count % INDEX_THROTTLE_EVERY == 0 {
            thread::sleep(INDEX_THROTTLE_SLEEP);
        }
    }

    writer
        .commit()
        .map_err(|error| search_index_error(index_dir, error))?;
    replace_index_dir(index_dir, &pending_index_dir)?;

    Ok(FileSearchIndexOutcome {
        root: root.to_path_buf(),
        index_dir: index_dir.to_path_buf(),
        indexed_count,
        skipped,
    })
}

fn search_file_index_blocking(
    index_dir: &Path,
    root: &Path,
    query: &str,
    options: FileSearchOptions,
) -> Result<FileSearchOutcome, FileError> {
    if !file_search_index_exists(index_dir) {
        return Err(search_index_error(root, "search index is not ready"));
    }

    let index = Index::open_in_dir(index_dir).map_err(|error| search_index_error(root, error))?;
    let schema = index.schema();
    let fields = FileSearchFields::from_schema(root, &schema)?;
    let boosts = tantivy_boosts_for_index(root, &index, fields, query)?;
    let candidates = candidates_from_index(root, &index, fields)?;
    let matches = nucleo_ranked_matches(query, &candidates, &boosts, options.limit.max(1));

    Ok(FileSearchOutcome {
        root: root.to_path_buf(),
        matches,
        skipped: Vec::new(),
    })
}

fn search_walk_builder(
    root: &Path,
    index_dir: &Path,
    options: &FileSearchIndexOptions,
) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    let excluded_index_dir = index_dir.to_path_buf();
    let excluded_pending_index_dir = index_dir.with_extension("building");
    builder
        .hidden(!options.include_hidden)
        .parents(true)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            !entry.path().starts_with(&excluded_index_dir)
                && !entry.path().starts_with(&excluded_pending_index_dir)
        });
    builder
}

fn prepare_index_dir(root: &Path, pending_index_dir: &Path) -> Result<(), FileError> {
    if let Some(parent) = pending_index_dir.parent() {
        std_fs::create_dir_all(parent).map_err(|error| search_index_error(root, error))?;
    }
    if pending_index_dir.exists() {
        std_fs::remove_dir_all(pending_index_dir)
            .map_err(|error| search_index_error(root, error))?;
    }
    std_fs::create_dir_all(pending_index_dir).map_err(|error| search_index_error(root, error))
}

fn replace_index_dir(index_dir: &Path, pending_index_dir: &Path) -> Result<(), FileError> {
    if index_dir.exists() {
        std_fs::remove_dir_all(index_dir).map_err(|error| search_index_error(index_dir, error))?;
    }
    std_fs::rename(pending_index_dir, index_dir)
        .map_err(|error| search_index_error(index_dir, error))
}

fn file_search_schema() -> (Schema, FileSearchFields) {
    let mut schema_builder = Schema::builder();
    let fields = FileSearchFields {
        name: schema_builder.add_text_field(SEARCH_FIELD_NAME, TEXT | STORED),
        path: schema_builder.add_text_field(SEARCH_FIELD_PATH, TEXT | STORED),
        path_key: schema_builder.add_text_field(SEARCH_FIELD_PATH_KEY, STRING | STORED),
        kind: schema_builder.add_text_field(SEARCH_FIELD_KIND, STRING | STORED),
    };
    (schema_builder.build(), fields)
}

fn search_document(fields: FileSearchFields, candidate: &SearchCandidate) -> TantivyDocument {
    doc!(
        fields.name => candidate.name_text.as_str(),
        fields.path => candidate.path_text.as_str(),
        fields.path_key => candidate.storage_key.as_str(),
        fields.kind => file_kind_key(candidate.kind),
    )
}

fn candidates_from_index(
    root: &Path,
    index: &Index,
    fields: FileSearchFields,
) -> Result<Vec<SearchCandidate>, FileError> {
    let reader = index
        .reader()
        .map_err(|error| search_index_error(root, error))?;
    let searcher = reader.searcher();
    let doc_addresses = searcher
        .search(&AllQuery, &DocSetCollector)
        .map_err(|error| search_index_error(root, error))?;
    let mut candidates = Vec::with_capacity(doc_addresses.len());

    for doc_address in doc_addresses {
        let document = searcher
            .doc::<TantivyDocument>(doc_address)
            .map_err(|error| search_index_error(root, error))?;
        if let Some(candidate) = candidate_from_document(root, fields, &document) {
            candidates.push(candidate);
        }
    }

    Ok(candidates)
}

fn candidate_from_document(
    root: &Path,
    fields: FileSearchFields,
    document: &TantivyDocument,
) -> Option<SearchCandidate> {
    let storage_key = document.get_first(fields.path_key)?.as_str()?.to_owned();
    let path = path_from_storage_key(&storage_key)?;
    let kind = document
        .get_first(fields.kind)
        .and_then(|value| value.as_str())
        .and_then(file_kind_from_key)
        .unwrap_or(FileKind::Other);
    let relative_path = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.clone());
    let name = path
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_else(|| path.as_os_str().to_os_string());
    let name_text = document
        .get_first(fields.name)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_string_lossy().into_owned());
    let path_text = document
        .get_first(fields.path)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| relative_path.to_string_lossy().into_owned());

    Some(SearchCandidate {
        path,
        relative_path,
        name,
        kind,
        name_text,
        path_text,
        storage_key,
    })
}

fn file_kind_from_metadata(metadata: &std_fs::Metadata) -> FileKind {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
    }
}

fn file_kind_key(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Directory => "directory",
        FileKind::File => "file",
        FileKind::Symlink => "symlink",
        FileKind::Other => "other",
    }
}

fn file_kind_from_key(key: &str) -> Option<FileKind> {
    match key {
        "directory" => Some(FileKind::Directory),
        "file" => Some(FileKind::File),
        "symlink" => Some(FileKind::Symlink),
        "other" => Some(FileKind::Other),
        _ => None,
    }
}

fn ignore_error_warning(root: &Path, error: ignore::Error) -> ScanWarning {
    let path = ignore_error_path(&error)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    ScanWarning {
        path,
        message: error.to_string(),
    }
}

fn ignore_error_path(error: &ignore::Error) -> Option<&Path> {
    match error {
        ignore::Error::Partial(errors) => errors.iter().find_map(ignore_error_path),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            ignore_error_path(err)
        }
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::Loop { child, .. } => Some(child),
        _ => None,
    }
}

#[cfg(unix)]
fn path_storage_key(path: &Path) -> String {
    hex_encode(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_storage_key(path: &Path) -> String {
    hex_encode(path.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn path_from_storage_key(value: &str) -> Option<PathBuf> {
    Some(PathBuf::from(OsString::from_vec(hex_decode(value)?)))
}

#[cfg(not(unix))]
fn path_from_storage_key(value: &str) -> Option<PathBuf> {
    String::from_utf8(hex_decode(value)?)
        .ok()
        .map(PathBuf::from)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }

    let mut decoded = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let high = hex_value(chunk[0])?;
        let low = hex_value(chunk[1])?;
        decoded.push((high << 4) | low);
    }
    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
