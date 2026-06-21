use std::path::PathBuf;

use file_core::FileKind;
use iced::Task;
use tokio_util::sync::CancellationToken;

use crate::app::FileBrowser;
use crate::commands::network_preview_cache_command;
use crate::model::{
    Message, NetworkPreviewCacheFinished, NetworkPreviewCacheMessage, NetworkPreviewCacheProgress,
    NetworkPreviewDownload, PreviewState, PreviewWindowProfile,
};
use crate::network_preview_cache::default_network_preview_cache_dir;

impl FileBrowser {
    pub(in crate::app) fn start_network_preview_download(
        &mut self,
        source_path: PathBuf,
    ) -> Task<Message> {
        let window_command = self.ensure_preview_window(PreviewWindowProfile::Regular);
        self.clear_preview();

        self.network_preview_download_generation =
            self.network_preview_download_generation.wrapping_add(1);
        let generation = self.network_preview_download_generation;
        let cancel = CancellationToken::new();
        self.network_preview_download_cancel = Some(cancel.clone());
        self.preview = Some(PreviewState::DownloadingNetworkFile(
            NetworkPreviewDownload::new(source_path.clone(), generation),
        ));
        self.error = None;

        Task::batch([
            window_command,
            network_preview_cache_command(
                source_path,
                generation,
                default_network_preview_cache_dir(),
                cancel,
            ),
        ])
    }

    pub(in crate::app) fn accept_network_preview_cache_message(
        &mut self,
        message: NetworkPreviewCacheMessage,
    ) -> Task<Message> {
        match message {
            NetworkPreviewCacheMessage::Progress(progress) => {
                self.accept_network_preview_cache_progress(progress);
                Task::none()
            }
            NetworkPreviewCacheMessage::Finished(finished) => {
                self.accept_network_preview_cache_finished(finished)
            }
        }
    }

    pub(super) fn cancel_network_preview_download(&mut self) {
        if let Some(cancel) = self.network_preview_download_cancel.take() {
            cancel.cancel();
        }
    }

    fn accept_network_preview_cache_progress(&mut self, progress: NetworkPreviewCacheProgress) {
        let Some(PreviewState::DownloadingNetworkFile(download)) = self.preview.as_mut() else {
            return;
        };
        if download.source_path != progress.source_path
            || download.generation != progress.generation
        {
            return;
        }

        download.accept_progress(&progress);
    }

    fn accept_network_preview_cache_finished(
        &mut self,
        finished: NetworkPreviewCacheFinished,
    ) -> Task<Message> {
        if !self.network_preview_download_matches(&finished.source_path, finished.generation) {
            return Task::none();
        }

        self.network_preview_download_cancel = None;
        match finished.outcome {
            Ok(cache_path) => self.open_preview_for_resolved_path(cache_path, FileKind::File),
            Err(error) => {
                self.text_preview_document = None;
                self.preview = Some(PreviewState::Error(format!(
                    "Could not download network preview: {error}"
                )));
                if self.preview_window.is_none() {
                    self.ensure_preview_window(PreviewWindowProfile::Regular)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn network_preview_download_matches(&self, source_path: &PathBuf, generation: u64) -> bool {
        matches!(
            &self.preview,
            Some(PreviewState::DownloadingNetworkFile(download))
                if download.source_path == *source_path && download.generation == generation
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::config;
    use crate::model::{NetworkPreviewCacheFinished, NetworkPreviewCacheProgress};

    use super::*;

    #[test]
    fn stale_network_preview_progress_does_not_update_active_download() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let active_path = PathBuf::from("/run/user/1000/gvfs/dav/active.txt");
        browser.preview = Some(PreviewState::DownloadingNetworkFile(
            NetworkPreviewDownload::new(active_path.clone(), 2),
        ));

        browser.accept_network_preview_cache_progress(NetworkPreviewCacheProgress {
            source_path: active_path,
            generation: 1,
            bytes_done: 50,
            bytes_total: 100,
        });

        let Some(PreviewState::DownloadingNetworkFile(download)) = browser.preview else {
            panic!("expected download state");
        };
        assert_eq!(download.bytes_done, 0);
        assert_eq!(download.bytes_total, None);
    }

    #[test]
    fn stale_network_preview_finished_does_not_replace_active_download() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let active_path = PathBuf::from("/run/user/1000/gvfs/dav/active.txt");
        browser.preview = Some(PreviewState::DownloadingNetworkFile(
            NetworkPreviewDownload::new(active_path.clone(), 2),
        ));

        let _ = browser.accept_network_preview_cache_finished(NetworkPreviewCacheFinished {
            source_path: active_path,
            generation: 1,
            outcome: Ok(PathBuf::from("/tmp/cache.txt")),
        });

        let Some(PreviewState::DownloadingNetworkFile(download)) = browser.preview else {
            panic!("expected download state");
        };
        assert_eq!(download.generation, 2);
    }
}
