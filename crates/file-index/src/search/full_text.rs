use std::path::{Path, PathBuf};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, TantivyDocument};
use tantivy::{IndexReader, IndexWriter};

use super::extractor::{ExtractedMediaDocument, ExtractedTextDocument};
use super::path_encoding::path_storage_key;
use super::types::{MediaSearchKind, MediaSearchMetadata, SearchResultSource};
use crate::IndexError;

const TANTIVY_DIR_NAME: &str = "tantivy";
const PATH_KEY_FIELD: &str = "path_key";
const PATH_FIELD: &str = "path";
const RELATIVE_PATH_FIELD: &str = "relative_path";
const NAME_FIELD: &str = "name";
const SOURCE_FIELD: &str = "source";
const BODY_FIELD: &str = "body";
const MEDIA_KIND_FIELD: &str = "media_kind";
const WIDTH_FIELD: &str = "width";
const HEIGHT_FIELD: &str = "height";
const DURATION_FIELD: &str = "duration_ms";
const CODEC_FIELD: &str = "codec";
const TITLE_FIELD: &str = "title";
const ARTIST_FIELD: &str = "artist";
const EXIF_FIELD: &str = "exif";
const RANK_HINT_FIELD: &str = "rank_hint";
const TANTIVY_WRITER_MEMORY_BUDGET_BYTES: usize = 15_000_000;

#[derive(Clone)]
struct SearchSchema {
    schema: Schema,
    path_key: Field,
    relative_path: Field,
    name: Field,
    source: Field,
    body: Field,
    media_kind: Field,
    width: Field,
    height: Field,
    duration_ms: Field,
    codec: Field,
    title: Field,
    artist: Field,
    exif: Field,
    rank_hint: Field,
}

pub(crate) struct TantivySearchRuntime {
    index: Index,
    reader: IndexReader,
    schema: SearchSchema,
    tantivy_dir: PathBuf,
}

pub(crate) struct TantivyIndexWriter {
    writer: IndexWriter,
    schema: SearchSchema,
    tantivy_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct FullTextSearchHit {
    pub(crate) storage_key: String,
    pub(crate) source: SearchResultSource,
    pub(crate) score: u32,
    pub(crate) snippet: Option<String>,
    pub(crate) media: Option<MediaSearchMetadata>,
}

impl TantivyIndexWriter {
    pub(crate) fn create(index_dir: &Path) -> Result<Self, IndexError> {
        let schema = search_schema();
        let tantivy_dir = tantivy_dir(index_dir);
        std::fs::create_dir_all(&tantivy_dir)
            .map_err(|error| IndexError::store(&tantivy_dir, error))?;
        let index = Index::create_in_dir(&tantivy_dir, schema.schema.clone())
            .map_err(|error| IndexError::store(&tantivy_dir, error))?;
        let writer = index
            .writer(TANTIVY_WRITER_MEMORY_BUDGET_BYTES)
            .map_err(|error| IndexError::store(&tantivy_dir, error))?;

        Ok(Self {
            writer,
            schema,
            tantivy_dir,
        })
    }

    pub(crate) fn add_text_document(
        &mut self,
        text: &ExtractedTextDocument,
    ) -> Result<(), IndexError> {
        let mut document = doc!(
            self.schema.path_key => path_storage_key(&text.path),
            self.schema.relative_path => text.relative_path.to_string_lossy().into_owned(),
            self.schema.name => text.name.clone(),
            self.schema.source => source_key(SearchResultSource::Contents),
            self.schema.body => text.content.clone(),
            self.schema.rank_hint => text.rank_hint,
        );
        if text.truncated {
            document.add_text(self.schema.body, " content truncated");
        }
        self.writer
            .add_document(document)
            .map(|_| ())
            .map_err(|error| IndexError::store(&self.tantivy_dir, error))
    }

    pub(crate) fn add_media_document(
        &mut self,
        media: &ExtractedMediaDocument,
    ) -> Result<(), IndexError> {
        let metadata = &media.metadata;
        let mut document = doc!(
            self.schema.path_key => path_storage_key(&media.path),
            self.schema.relative_path => media.relative_path.to_string_lossy().into_owned(),
            self.schema.name => media.name.clone(),
            self.schema.source => source_key(SearchResultSource::Media),
            self.schema.body => media.searchable_text.clone(),
            self.schema.media_kind => media_kind_key(metadata.media_kind),
            self.schema.rank_hint => media.rank_hint,
        );
        add_optional_u64(
            &mut document,
            self.schema.width,
            metadata.width.map(u64::from),
        );
        add_optional_u64(
            &mut document,
            self.schema.height,
            metadata.height.map(u64::from),
        );
        add_optional_u64(&mut document, self.schema.duration_ms, metadata.duration_ms);
        add_optional_text(&mut document, self.schema.codec, metadata.codec.as_deref());
        add_optional_text(&mut document, self.schema.title, metadata.title.as_deref());
        add_optional_text(
            &mut document,
            self.schema.artist,
            metadata.artist.as_deref(),
        );
        for exif in &metadata.exif {
            document.add_text(self.schema.exif, format!("{}\t{}", exif.tag, exif.value));
        }
        self.writer
            .add_document(document)
            .map(|_| ())
            .map_err(|error| IndexError::store(&self.tantivy_dir, error))
    }

    pub(crate) fn finish(mut self) -> Result<(), IndexError> {
        self.writer
            .commit()
            .map(|_| ())
            .map_err(|error| IndexError::store(&self.tantivy_dir, error))
    }
}

impl TantivySearchRuntime {
    pub(crate) fn open(index_dir: &Path) -> Result<Option<Self>, IndexError> {
        let tantivy_dir = tantivy_dir(index_dir);
        if !tantivy_dir.join("meta.json").is_file() {
            return Ok(None);
        }

        let schema = search_schema();
        let index = Index::open_in_dir(&tantivy_dir)
            .map_err(|error| IndexError::store(&tantivy_dir, error))?;
        let reader = index
            .reader()
            .map_err(|error| IndexError::store(&tantivy_dir, error))?;
        Ok(Some(Self {
            index,
            reader,
            schema,
            tantivy_dir,
        }))
    }

    fn query_parser(&self) -> QueryParser {
        QueryParser::for_index(
            &self.index,
            vec![
                self.schema.body,
                self.schema.name,
                self.schema.relative_path,
                self.schema.codec,
                self.schema.title,
                self.schema.artist,
                self.schema.exif,
            ],
        )
    }
}

pub(crate) fn search_tantivy_index(
    runtime: Option<&TantivySearchRuntime>,
    query_text: &str,
    sources: &[SearchResultSource],
    limit: usize,
) -> Result<Vec<FullTextSearchHit>, IndexError> {
    let Some(runtime) = runtime else {
        return Ok(Vec::new());
    };

    let searcher = runtime.reader.searcher();
    let query_parser = runtime.query_parser();
    let query = query_parser
        .parse_query(query_text)
        .map_err(|error| IndexError::store(&runtime.tantivy_dir, error))?;
    let top_docs = searcher
        .search(&query, &TopDocs::with_limit(limit.max(1)).order_by_score())
        .map_err(|error| IndexError::store(&runtime.tantivy_dir, error))?;
    let allowed_sources = sources.iter().copied().map(source_key).collect::<Vec<_>>();
    let mut hits = Vec::new();

    for (score, address) in top_docs {
        let document = searcher
            .doc::<TantivyDocument>(address)
            .map_err(|error| IndexError::store(&runtime.tantivy_dir, error))?;
        let Some(source_text) = first_text(&document, runtime.schema.source) else {
            continue;
        };
        if !allowed_sources.iter().any(|source| *source == source_text) {
            continue;
        }
        let Some(source) = source_from_key(source_text) else {
            continue;
        };
        let Some(storage_key) = first_text(&document, runtime.schema.path_key) else {
            continue;
        };

        hits.push(FullTextSearchHit {
            storage_key: storage_key.to_owned(),
            source,
            score: score_to_rank(score),
            snippet: None,
            media: media_metadata_from_document(&document, &runtime.schema, source),
        });
    }

    Ok(hits)
}

fn search_schema() -> SearchSchema {
    let mut builder = Schema::builder();
    let path_key = builder.add_text_field(PATH_KEY_FIELD, STRING | STORED);
    builder.add_text_field(PATH_FIELD, STRING);
    let relative_path = builder.add_text_field(RELATIVE_PATH_FIELD, TEXT);
    let name = builder.add_text_field(NAME_FIELD, TEXT);
    let source = builder.add_text_field(SOURCE_FIELD, STRING | STORED);
    let body = builder.add_text_field(BODY_FIELD, TEXT);
    let media_kind = builder.add_text_field(MEDIA_KIND_FIELD, STRING | STORED);
    let width = builder.add_u64_field(WIDTH_FIELD, STORED);
    let height = builder.add_u64_field(HEIGHT_FIELD, STORED);
    let duration_ms = builder.add_u64_field(DURATION_FIELD, STORED);
    let codec = builder.add_text_field(CODEC_FIELD, TEXT | STORED);
    let title = builder.add_text_field(TITLE_FIELD, TEXT | STORED);
    let artist = builder.add_text_field(ARTIST_FIELD, TEXT | STORED);
    let exif = builder.add_text_field(EXIF_FIELD, TEXT | STORED);
    let rank_hint = builder.add_u64_field(RANK_HINT_FIELD, STORED);
    let schema = builder.build();
    SearchSchema {
        schema,
        path_key,
        relative_path,
        name,
        source,
        body,
        media_kind,
        width,
        height,
        duration_ms,
        codec,
        title,
        artist,
        exif,
        rank_hint,
    }
}

fn tantivy_dir(index_dir: &Path) -> PathBuf {
    index_dir.join(TANTIVY_DIR_NAME)
}

fn add_optional_text(document: &mut TantivyDocument, field: Field, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        document.add_text(field, value);
    }
}

fn add_optional_u64(document: &mut TantivyDocument, field: Field, value: Option<u64>) {
    if let Some(value) = value {
        document.add_u64(field, value);
    }
}

fn first_text(document: &TantivyDocument, field: Field) -> Option<&str> {
    document.get_first(field).and_then(|value| value.as_str())
}

fn first_u64(document: &TantivyDocument, field: Field) -> Option<u64> {
    document.get_first(field).and_then(|value| value.as_u64())
}

fn media_metadata_from_document(
    document: &TantivyDocument,
    schema: &SearchSchema,
    source: SearchResultSource,
) -> Option<MediaSearchMetadata> {
    if source != SearchResultSource::Media {
        return None;
    }
    let media_kind = first_text(document, schema.media_kind).and_then(media_kind_from_key)?;
    Some(MediaSearchMetadata {
        media_kind,
        width: first_u64(document, schema.width).and_then(|value| u32::try_from(value).ok()),
        height: first_u64(document, schema.height).and_then(|value| u32::try_from(value).ok()),
        duration_ms: first_u64(document, schema.duration_ms),
        codec: first_text(document, schema.codec).map(ToOwned::to_owned),
        title: first_text(document, schema.title).map(ToOwned::to_owned),
        artist: first_text(document, schema.artist).map(ToOwned::to_owned),
        exif: document
            .get_all(schema.exif)
            .filter_map(|value| value.as_str())
            .filter_map(media_exif_from_text)
            .collect(),
    })
}

fn media_exif_from_text(text: &str) -> Option<super::types::MediaExifField> {
    let (tag, value) = text.split_once('\t')?;
    (!tag.is_empty() && !value.is_empty()).then(|| super::types::MediaExifField {
        tag: tag.to_owned(),
        value: value.to_owned(),
    })
}

fn source_key(source: SearchResultSource) -> &'static str {
    match source {
        SearchResultSource::Files => "files",
        SearchResultSource::Contents => "contents",
        SearchResultSource::Media => "media",
    }
}

fn source_from_key(key: &str) -> Option<SearchResultSource> {
    match key {
        "files" => Some(SearchResultSource::Files),
        "contents" => Some(SearchResultSource::Contents),
        "media" => Some(SearchResultSource::Media),
        _ => None,
    }
}

fn media_kind_key(kind: MediaSearchKind) -> &'static str {
    match kind {
        MediaSearchKind::Image => "image",
        MediaSearchKind::Audio => "audio",
        MediaSearchKind::Video => "video",
    }
}

fn media_kind_from_key(key: &str) -> Option<MediaSearchKind> {
    match key {
        "image" => Some(MediaSearchKind::Image),
        "audio" => Some(MediaSearchKind::Audio),
        "video" => Some(MediaSearchKind::Video),
        _ => None,
    }
}

fn score_to_rank(score: f32) -> u32 {
    if score.is_finite() && score > 0.0 {
        (score * 1000.0).round().min(u32::MAX as f32) as u32
    } else {
        0
    }
}
