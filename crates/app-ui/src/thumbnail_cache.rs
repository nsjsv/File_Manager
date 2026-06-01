use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use file_core::{DirectoryEntry, FileKind};
use iced::widget::image;
use thumbnails::{CachedThumbnail, ThumbnailKey, ThumbnailRequest, ThumbnailSourceMetadata};

pub(crate) const LIST_THUMBNAIL_EDGE: u32 = 96;
pub(crate) const LIST_THUMBNAIL_SIZE: f32 = 42.0;
pub(crate) const PREVIEW_THUMBNAIL_MAX_EDGE: u32 = 2048;

const MAX_READY_THUMBNAILS: usize = 1200;
const MAX_IN_FLIGHT: usize = 2;
const FAILURE_BACKOFF: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbnailPurpose {
    List,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ThumbnailPriority {
    Background,
    Visible,
    Focused,
    Preview,
}

#[derive(Debug, Clone)]
pub(crate) struct ThumbnailWork {
    pub(crate) request: ThumbnailRequest,
    pub(crate) purpose: ThumbnailPurpose,
    pub(crate) priority: ThumbnailPriority,
}

impl ThumbnailWork {
    pub(crate) fn key(&self) -> ThumbnailKey {
        self.request.key()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ThumbnailLoadOutcome {
    pub(crate) work: ThumbnailWork,
    pub(crate) result: Result<CachedThumbnail, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ThumbnailHandleEntry {
    pub(crate) handle: image::Handle,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) max_edge: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ColumnViewport {
    pub(crate) offset_y: f32,
    pub(crate) height: f32,
}

#[derive(Debug)]
pub(crate) struct ThumbnailCache {
    cache_dir: PathBuf,
    ready: HashMap<ThumbnailKey, ThumbnailHandleEntry>,
    ready_order: VecDeque<ThumbnailKey>,
    queued: HashMap<ThumbnailKey, ThumbnailWork>,
    queue_order: VecDeque<ThumbnailKey>,
    inflight: HashSet<ThumbnailKey>,
    failures: HashMap<ThumbnailKey, Instant>,
}

impl ThumbnailCache {
    pub(crate) fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            ready: HashMap::new(),
            ready_order: VecDeque::new(),
            queued: HashMap::new(),
            queue_order: VecDeque::new(),
            inflight: HashSet::new(),
            failures: HashMap::new(),
        }
    }

    pub(crate) fn set_cache_dir(&mut self, cache_dir: PathBuf) {
        if self.cache_dir == cache_dir {
            return;
        }
        self.cache_dir = cache_dir;
        self.ready.clear();
        self.ready_order.clear();
        self.queued.clear();
        self.queue_order.clear();
        self.inflight.clear();
        self.failures.clear();
    }

    pub(crate) fn cache_dir(&self) -> PathBuf {
        self.cache_dir.clone()
    }

    pub(crate) fn enqueue_entry(
        &mut self,
        entry: &DirectoryEntry,
        max_edge: u32,
        purpose: ThumbnailPurpose,
        priority: ThumbnailPriority,
    ) {
        let Some(request) = request_for_entry(entry, max_edge) else {
            return;
        };
        self.enqueue_request(request, purpose, priority);
    }

    pub(crate) fn enqueue_request(
        &mut self,
        request: ThumbnailRequest,
        purpose: ThumbnailPurpose,
        priority: ThumbnailPriority,
    ) {
        let key = request.key();
        if self.ready.contains_key(&key) || self.inflight.contains(&key) {
            return;
        }
        if self.failure_is_active(&key) {
            return;
        }

        let work = ThumbnailWork {
            request,
            purpose,
            priority,
        };
        if let Some(existing) = self.queued.get_mut(&key) {
            if existing.priority < priority {
                *existing = work;
            }
            return;
        }

        self.queued.insert(key.clone(), work);
        self.queue_order.push_back(key);
    }

    pub(crate) fn take_next_batch(&mut self) -> Vec<ThumbnailWork> {
        let available = MAX_IN_FLIGHT.saturating_sub(self.inflight.len());
        if available == 0 || self.queued.is_empty() {
            return Vec::new();
        }

        let mut works = Vec::new();
        for _ in 0..available {
            let Some(key) = self.pop_highest_priority_key() else {
                break;
            };
            let Some(work) = self.queued.remove(&key) else {
                continue;
            };
            self.inflight.insert(key);
            works.push(work);
        }
        works
    }

    pub(crate) fn finish(&mut self, key: &ThumbnailKey) {
        self.inflight.remove(key);
    }

    pub(crate) fn insert_ready(
        &mut self,
        thumbnail: CachedThumbnail,
        max_edge: u32,
    ) -> ThumbnailHandleEntry {
        let key = thumbnail.key.clone();
        let entry = ThumbnailHandleEntry {
            handle: image::Handle::from_path(thumbnail.output.clone()),
            width: thumbnail.width,
            height: thumbnail.height,
            max_edge,
        };

        self.ready.insert(key.clone(), entry.clone());
        self.ready_order.retain(|ready_key| ready_key != &key);
        self.ready_order.push_back(key);
        self.trim_ready_cache();
        entry
    }

    pub(crate) fn ready_for_entry(
        &self,
        entry: &DirectoryEntry,
        max_edge: u32,
    ) -> Option<&ThumbnailHandleEntry> {
        let request = request_for_entry(entry, max_edge)?;
        self.ready.get(&request.key())
    }

    pub(crate) fn ready_for_request(
        &self,
        request: &ThumbnailRequest,
    ) -> Option<&ThumbnailHandleEntry> {
        self.ready.get(&request.key())
    }

    pub(crate) fn mark_failure(&mut self, key: ThumbnailKey) {
        self.failures.insert(key, Instant::now());
    }

    fn failure_is_active(&mut self, key: &ThumbnailKey) -> bool {
        let Some(failed_at) = self.failures.get(key).copied() else {
            return false;
        };
        if failed_at.elapsed() <= FAILURE_BACKOFF {
            return true;
        }
        self.failures.remove(key);
        false
    }

    fn pop_highest_priority_key(&mut self) -> Option<ThumbnailKey> {
        let mut best_index = None;
        let mut best_priority = ThumbnailPriority::Background;

        for (index, key) in self.queue_order.iter().enumerate() {
            let Some(work) = self.queued.get(key) else {
                continue;
            };
            if best_index.is_none() || work.priority > best_priority {
                best_index = Some(index);
                best_priority = work.priority;
            }
        }

        let index = best_index?;
        self.queue_order.remove(index)
    }

    fn trim_ready_cache(&mut self) {
        while self.ready.len() > MAX_READY_THUMBNAILS {
            let Some(key) = self.ready_order.pop_front() else {
                break;
            };
            self.ready.remove(&key);
        }
    }
}

pub(crate) fn request_for_entry(entry: &DirectoryEntry, max_edge: u32) -> Option<ThumbnailRequest> {
    if entry.kind != FileKind::File || !thumbnails::is_supported_image_path(&entry.path) {
        return None;
    }

    Some(ThumbnailRequest::new(
        &entry.path,
        ThumbnailSourceMetadata::from(&entry.metadata),
        max_edge,
    ))
}

pub(crate) fn is_supported_image_path(path: &Path) -> bool {
    thumbnails::is_supported_image_path(path)
}
