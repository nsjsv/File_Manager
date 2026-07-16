use iced::Task;

use super::FileBrowser;
use crate::commands::search_service_status_command;
use crate::model::{Message, NavigationMode, OperationQueuePanelMode, ScrollbarRegion as Region};

impl FileBrowser {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartupEnvironmentLoaded(startup_environment) => {
                self.accept_startup_environment(startup_environment)
            }
            Message::SidebarLocationsLoaded(sidebar_locations) => {
                self.accept_sidebar_locations(sidebar_locations)
            }
            Message::SidebarDevicesLoaded(devices) => self.accept_sidebar_devices(devices),
            Message::SidebarDevicesRefreshRequested => self.refresh_sidebar_devices(),
            Message::SidebarDeviceHovered(id) => self.handle_sidebar_device_hovered(id),
            Message::SidebarDeviceHoverCleared(id) => self.handle_sidebar_device_hover_cleared(id),
            Message::SidebarDevicePressed(id) => self.handle_sidebar_device_pressed(id),
            Message::SidebarDeviceMiddlePressed(pane_id, id) => {
                self.handle_sidebar_device_middle_pressed(pane_id, id)
            }
            Message::SidebarDeviceRightClicked(id) => self.handle_sidebar_device_right_clicked(id),
            Message::SidebarDeviceActionSelected(id, action) => {
                self.perform_sidebar_device_action(id, action)
            }
            Message::SidebarDeviceActionFinished(id, action, result) => {
                self.accept_sidebar_device_action_finished(id, action, result)
            }
            Message::NetworkConnection(message) => self.handle_network_connection_message(message),
            Message::OperationStoreLoaded(operation_store) => {
                self.accept_operation_store(operation_store)
            }
            Message::DirectoryLoadBatch(request, batch) => {
                self.accept_directory_scan_batch(request, batch)
            }
            Message::Loaded(request, Ok(scan)) => self.accept_directory_scan(request, scan),
            Message::Loaded(request, Err(error)) => {
                if request.pane_id == self.active_pane_id() {
                    if request.generation != self.directory_load_generation
                        || request.path != self.current_dir
                    {
                        return Task::none();
                    }
                    self.is_loading = false;
                    self.directory_loading_placeholder_entries.clear();
                    self.directory_load_cancel = None;
                } else if let Some(pane) = self.pane_by_id_mut(request.pane_id) {
                    if request.generation != pane.directory_load_generation
                        || request.path != pane.current_dir
                    {
                        return Task::none();
                    }
                    pane.is_loading = false;
                    pane.directory_loading_placeholder_entries.clear();
                    pane.directory_load_cancel = None;
                }
                self.show_global_error(error);
                Task::none()
            }
            Message::TrashLoaded(pane_id, Ok(scan)) => self.accept_trash_scan(pane_id, scan),
            Message::TrashLoaded(pane_id, Err(error)) => {
                if pane_id == self.active_pane_id() {
                    self.is_loading = false;
                    self.directory_loading_placeholder_entries.clear();
                } else if let Some(pane) = self.pane_by_id_mut(pane_id) {
                    pane.is_loading = false;
                    pane.directory_loading_placeholder_entries.clear();
                }
                self.show_global_error(error);
                Task::none()
            }
            Message::OpenFileFinished(_, Ok(())) => {
                self.clear_global_error();
                Task::none()
            }
            Message::OpenFileFinished(path, Err(error)) => {
                self.request_open_with_after_default_open_failed(path, error)
            }
            Message::OpenWithRequested(path) => self.request_open_with_applications(path),
            Message::OpenWithApplicationsLoaded(path, applications) => {
                self.accept_open_with_applications(path, applications)
            }
            Message::OpenWithDefaultApplicationToggled(selected) => {
                self.toggle_open_with_default_application(selected)
            }
            Message::OpenWithApplicationSelected(desktop_id) => {
                self.select_open_with_application(desktop_id)
            }
            Message::OpenWithApplicationFinished(result) => {
                self.accept_open_with_application_finished(result)
            }
            Message::OpenTerminalFinished(Ok(())) => {
                self.clear_global_error();
                Task::none()
            }
            Message::OpenTerminalFinished(Err(error)) => {
                self.show_global_error(error);
                Task::none()
            }
            Message::PreviewLoaded(path, preview_outcome) => {
                self.accept_preview(path, preview_outcome)
            }
            Message::NetworkPreviewCache(message) => {
                self.accept_network_preview_cache_message(message)
            }
            Message::AnimatedImagePreviewLoaded(path, generation, preview_outcome) => {
                self.accept_animated_image_preview_loaded(path, generation, preview_outcome)
            }
            Message::FilePropertiesLoaded(request, properties_outcome) => {
                self.accept_file_properties(request, properties_outcome)
            }
            Message::FilePropertiesDirectoryContentsUpdated(request, contents) => {
                self.accept_file_properties_directory_contents_progress(request, contents)
            }
            Message::FilePropertiesDirectoryContentsLoaded(request, contents_outcome) => {
                self.accept_file_properties_directory_contents(request, contents_outcome)
            }
            Message::FilePropertiesCategorySelected(category) => {
                self.select_file_properties_category(category)
            }
            Message::FilePropertiesPermissionToggled(class, access) => {
                self.toggle_file_properties_permission(class, access)
            }
            Message::FilePropertiesApplyPermissionsToEnclosedItems => {
                self.apply_file_properties_permissions_to_enclosed_items()
            }
            Message::FilePropertiesPermissionsUpdated(request, permissions_outcome) => {
                self.accept_file_properties_permissions(request, permissions_outcome)
            }
            Message::FilePropertiesEnclosedPermissionsUpdated(request, permissions_outcome) => {
                self.accept_file_properties_enclosed_permissions(request, permissions_outcome)
            }
            Message::PreviewDirectoryChildrenLoaded(parent_path, children_outcome) => {
                self.accept_preview_directory_children(parent_path, children_outcome)
            }
            Message::TextPreviewAction {
                action,
                viewport_height,
            } => self.handle_text_preview_action(action, viewport_height),
            Message::TextPreviewChunkLoaded {
                path,
                generation,
                start_offset,
                outcome,
            } => self.accept_text_preview_chunk(path, generation, start_offset, outcome),
            Message::MarkdownPreviewScrolled {
                offset_y,
                viewport_height,
                content_height,
            } => Task::batch([
                self.show_scrollbars_temporarily(Region::MarkdownPreview),
                self.handle_markdown_preview_scrolled(offset_y, viewport_height, content_height),
            ]),
            Message::MarkdownPreviewModeSelected(mode) => {
                if let Some(document) = self.text_preview_document.as_mut() {
                    document.select_markdown_preview_mode(mode);
                }
                Task::none()
            }
            Message::ImagePreviewDimensionsLoaded(path, dimensions_outcome) => {
                self.accept_image_preview_dimensions(path, dimensions_outcome)
            }
            Message::AnimatedImageFrameLoaded(frame) => self.accept_animated_image_frame(frame),
            Message::AnimatedImagePreviewFinished(path, generation) => {
                self.accept_animated_image_preview_finished(path, generation)
            }
            Message::AnimatedImagePreviewFailed(path, generation, error) => {
                self.accept_animated_image_preview_error(path, generation, error)
            }
            Message::AnimatedImageSeekRequested(position) => {
                self.seek_animated_image_preview(position)
            }
            Message::AnimatedImageSeekCommitted => self.commit_animated_image_preview_seek(),
            Message::AudioPreviewPlaybackToggled => self.toggle_audio_preview_playback(),
            Message::AudioPreviewStarted(path, playback_outcome) => {
                self.accept_audio_preview_started(path, playback_outcome)
            }
            Message::AudioPreviewSeekRequested(position) => {
                self.seek_audio_preview_playback(position)
            }
            Message::AudioPreviewVolumeChanged(volume) => self.change_audio_preview_volume(volume),
            Message::AudioPreviewTick => self.update_audio_preview_playback(),
            Message::VideoPreviewPlaybackToggled => self.toggle_video_preview_playback(),
            Message::VideoPreviewAudioStarted(path, generation, audio_outcome) => {
                self.accept_video_preview_audio_started(path, generation, audio_outcome)
            }
            Message::VideoPreviewMetadataLoaded(path, metadata_outcome) => {
                self.accept_video_preview_metadata(path, metadata_outcome)
            }
            Message::VideoPreviewSeekRequested(position) => {
                self.seek_video_preview_playback(position)
            }
            Message::VideoPreviewSeekCommitted => self.commit_video_preview_seek(),
            Message::VideoPreviewVolumeChanged(volume) => self.change_video_preview_volume(volume),
            Message::VideoPreviewTick => self.update_video_preview_playback(),
            Message::VideoPreviewFrameLoaded(frame) => self.accept_video_preview_frame(frame),
            Message::VideoPreviewSeekFrameFailed(path, generation, position, error) => {
                self.accept_video_preview_seek_frame_error(path, generation, position, error)
            }
            Message::VideoPreviewFinished(path, generation) => {
                self.accept_video_preview_finished(path, generation)
            }
            Message::VideoPreviewFailed(path, generation, error) => {
                self.accept_video_preview_error(path, generation, error)
            }
            Message::FileOperationProgressed(task_id, progress) => {
                if let Some(error) = self.operation_queue.update_progress(task_id, progress) {
                    self.show_global_error(error);
                }
                Task::none()
            }
            Message::FileOperationFinished(task_id, result) => {
                self.accept_file_operation_finished(task_id, result)
            }
            Message::FileOperationIndicatorPressed => {
                self.context_menu = None;
                self.open_with = None;
                if self.operation_queue.is_panel_open()
                    && self.operation_queue_panel_mode == OperationQueuePanelMode::InteractiveList
                {
                    self.operation_queue.close_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
                } else {
                    self.operation_queue.open_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::InteractiveList;
                }
                self.operation_queue_auto_hide_generation =
                    self.operation_queue_auto_hide_generation.wrapping_add(1);
                Task::none()
            }
            Message::FileOperationAutoHideElapsed(generation) => {
                if generation == self.operation_queue_auto_hide_generation {
                    self.operation_queue.close_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
                }
                Task::none()
            }
            Message::FileOperationPauseToggled(task_id) => {
                if let Some(error) = self.operation_queue.toggle_pause(task_id) {
                    self.show_global_error(error);
                }
                Task::none()
            }
            Message::FileOperationCancelRequested(task_id) => {
                if let Some(error) = self.operation_queue.cancel(task_id) {
                    self.show_global_error(error);
                }
                Task::none()
            }
            Message::PreviewTreeDirectoryToggled(entry_id) => {
                self.toggle_preview_tree_directory(entry_id)
            }
            Message::PreviewTreeAnimationTick => self.advance_preview_tree_animation(),
            Message::ThumbnailRefreshRequested(pane_id, directory) => {
                self.accept_thumbnail_refresh_request(pane_id, directory)
            }
            Message::ThumbnailBatchLoaded(outcomes) => self.accept_thumbnail_batch(outcomes),
            Message::BrowserViewModeSelected(pane_id, view_mode) => {
                self.select_browser_view_mode(pane_id, view_mode)
            }
            Message::ListDirectoryToggled(pane_id, path) => {
                self.toggle_list_directory(pane_id, path)
            }
            Message::FlatEntryClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                if self.pane_drag.is_some() || self.ctrl_shift_pane_drag_shortcut_is_pressed() {
                    return Task::none();
                }
                self.handle_flat_entry_clicked(path)
            }
            Message::ListHeaderRightClicked(pane_id) => self.open_list_column_menu(pane_id),
            Message::ListColumnVisibilityToggled(column) => {
                self.toggle_list_column_visibility(column)
            }
            Message::ListColumnResizeStarted(pane_id, column) => {
                self.start_list_column_resize_drag(pane_id, column)
            }
            Message::ListColumnReorderStarted(pane_id, column) => {
                self.start_list_column_reorder_drag(pane_id, column)
            }
            Message::ListColumnReorderTargetEntered(column) => {
                self.enter_list_column_reorder_target(column)
            }
            Message::ListColumnReorderTargetExited(column) => {
                self.exit_list_column_reorder_target(column)
            }
            Message::ListDirectorySummaryLoaded(request, outcome) => {
                self.accept_list_directory_summary(request, outcome)
            }
            Message::ColumnEntryClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                if self.pane_drag.is_some() || self.ctrl_shift_pane_drag_shortcut_is_pressed() {
                    return Task::none();
                }
                self.handle_column_entry_clicked(path)
            }
            Message::ColumnBlankClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                if self.pane_drag.is_some() || self.ctrl_shift_pane_drag_shortcut_is_pressed() {
                    return Task::none();
                }
                self.start_column_blank_selection_marquee(path)
            }
            Message::ColumnPlaceholderPressed(pane_id) => {
                self.activate_pane(pane_id);
                if self.pane_drag.is_some() || self.ctrl_shift_pane_drag_shortcut_is_pressed() {
                    return Task::none();
                }
                self.handle_column_placeholder_pressed()
            }
            Message::EntryReleased(pane_id, path) => {
                let releasing_file_drag = self.file_drag.is_some();
                let release_directory = releasing_file_drag
                    .then(|| self.file_drag_release_directory_for_entry(pane_id, &path))
                    .flatten();
                if !releasing_file_drag {
                    self.activate_pane(pane_id);
                }
                self.finish_tab_drag();
                self.finish_pane_drag();
                Task::batch([
                    self.finish_sidebar_bookmark_drag(),
                    self.finish_sidebar_resize_drag_command(),
                    self.finish_column_resize_drag_command(),
                    self.finish_list_column_resize_drag_command(),
                    self.finish_list_column_reorder_drag_command(),
                    self.finish_drag_selection(release_directory),
                    self.schedule_thumbnail_refresh(),
                    self.request_breadcrumb_drop_target_bounds_measurement(),
                ])
            }
            Message::EntryRightClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                self.handle_entry_right_clicked(path)
            }
            Message::EntryHovered(pane_id, path) => {
                if pane_id == self.active_pane_id() {
                    self.handle_entry_hovered(path)
                } else if self.file_drag.is_some() {
                    self.handle_file_drag_entry_hovered_in_pane(pane_id, path)
                } else {
                    Task::none()
                }
            }
            Message::EntryHoverCleared(pane_id, path) => {
                if pane_id == self.active_pane_id() {
                    self.handle_entry_hover_cleared(path)
                } else if self.file_drag.is_some() {
                    self.handle_file_drag_entry_hover_cleared_in_pane(pane_id, path)
                } else {
                    Task::none()
                }
            }
            Message::DropTargetHovered(pane_id, directory) => {
                if pane_id == self.active_pane_id() {
                    self.handle_drop_target_hovered(directory)
                } else if self.file_drag.is_some() {
                    self.handle_file_drag_drop_target_hovered_in_pane(pane_id, directory)
                } else {
                    Task::none()
                }
            }
            Message::DropTargetHoverCleared(pane_id, directory) => {
                if pane_id == self.active_pane_id() {
                    self.handle_drop_target_hover_cleared(directory)
                } else if self.file_drag.is_some() {
                    self.handle_file_drag_drop_target_hover_cleared_in_pane(pane_id, directory)
                } else {
                    Task::none()
                }
            }
            Message::DropTargetReleased(pane_id, directory) => {
                let release_directory = if self.file_drag.is_some() {
                    self.file_drag_release_directory_for_drop_target(pane_id, directory)
                } else {
                    None
                };
                self.finish_tab_drag();
                self.finish_pane_drag();
                Task::batch([
                    self.finish_sidebar_bookmark_drag(),
                    self.finish_sidebar_resize_drag_command(),
                    self.finish_column_resize_drag_command(),
                    self.finish_list_column_resize_drag_command(),
                    self.finish_list_column_reorder_drag_command(),
                    self.finish_drag_selection(release_directory),
                    self.schedule_thumbnail_refresh(),
                    self.request_breadcrumb_drop_target_bounds_measurement(),
                ])
            }
            Message::BlankAreaPressed(pane_id) => {
                self.activate_pane(pane_id);
                if self.pane_drag.is_some() || self.ctrl_shift_pane_drag_shortcut_is_pressed() {
                    return Task::none();
                }
                self.start_selection_marquee()
            }
            Message::BlankAreaRightClicked(pane_id, directory) => {
                self.activate_pane(pane_id);
                self.handle_blank_area_right_clicked(directory)
            }
            Message::SidebarHovered(path) => self.handle_sidebar_hovered(path),
            Message::SidebarHoverCleared(path) => self.handle_sidebar_hover_cleared(path),
            Message::SidebarPointerMoved(position) => {
                self.update_sidebar_bookmark_drop_slot(position)
            }
            Message::SidebarPointerExited => self.clear_sidebar_bookmark_drop_slot(),
            Message::SidebarBookmarkPressed(path) => self.start_sidebar_bookmark_drag(path),
            Message::SidebarBookmarkRightClicked(path) => {
                self.handle_sidebar_bookmark_right_clicked(path)
            }
            Message::SidebarBookmarkEntered(path) => self.handle_sidebar_bookmark_entered(path),
            Message::SidebarBookmarkReleased => self.finish_sidebar_bookmark_drag(),
            Message::SidebarBookmarkDeleteRequested(path) => self.delete_sidebar_bookmark(path),
            Message::SidebarResizeStarted => self.start_sidebar_resize_drag(),
            Message::CursorMoved { window, position } => {
                self.update_pointer_motion(window, position)
            }
            Message::CursorLeft(window) => {
                if window == self.main_window {
                    self.request_file_drag_wayland_dnd_on_window_exit()
                } else {
                    Task::none()
                }
            }
            Message::ColumnBrowserCursorEntered(pane_id) => {
                if self.file_drag.is_none() {
                    self.activate_pane(pane_id);
                }
                self.is_cursor_over_column_browser = true;
                Task::none()
            }
            Message::ColumnBrowserCursorExited(pane_id) => {
                if pane_id == self.active_pane_id() {
                    self.is_cursor_over_column_browser = false;
                    self.clear_cursor_paste_target()
                } else {
                    Task::none()
                }
            }
            Message::ColumnEntryBoundsMeasured(bounds) => {
                self.update_selection_from_column_entry_bounds(bounds)
            }
            Message::BreadcrumbDropTargetBoundsMeasured(generation, bounds) => {
                self.accept_breadcrumb_drop_target_bounds(generation, bounds)
            }
            Message::PaneCursorEntered(pane_id) => {
                self.hovered_pane_id = Some(pane_id);
                Task::none()
            }
            Message::PaneCursorExited(pane_id) => {
                if self.hovered_pane_id == Some(pane_id) {
                    self.hovered_pane_id = None;
                }
                Task::none()
            }
            Message::KeyboardModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
                if self.promote_ctrl_shift_pane_drag_from_active_pointer_drag() {
                    self.update_pane_drag(self.cursor_position);
                }
                Task::none()
            }
            Message::KeyboardKeyPressed {
                key,
                modifiers,
                status,
            } => self.handle_keyboard_key_pressed(key, modifiers, status),
            Message::FileContentShortcutRouted(action) => self.invoke_shortcut(action),
            Message::ShortcutCaptureStarted(binding_id) => self.start_shortcut_capture(binding_id),
            Message::ShortcutCaptureCanceled => self.cancel_shortcut_capture(),
            Message::ShortcutBindingReset(binding_id) => self.reset_shortcut_binding(binding_id),
            Message::DragSelectionFinished => self.finish_pointer_drag_interactions(),
            Message::DismissFloating => self.dismiss_floating(),
            Message::ArchiveCreation(message) => self.handle_archive_creation_message(message),
            Message::ArchiveExtraction(message) => self.handle_archive_extraction_message(message),
            Message::BatchRename(message) => self.handle_batch_rename_message(message),
            Message::FileContextMenuExpansionChanged(expansion) => {
                self.update_file_context_menu_expansion(expansion)
            }
            Message::DestructiveActionConfirmed => self.confirm_destructive_action(),
            Message::DestructiveActionCanceled => self.cancel_destructive_action(),
            Message::AuxiliaryWindowCloseRequested(window) => self.close_auxiliary_window(window),
            Message::AuxiliaryWindowResized(window, width, height) => {
                self.handle_auxiliary_window_resized(window, width, height)
            }
            Message::WindowFocused(window) => self.handle_window_focused(window),
            Message::WindowUnfocused(window) => self.handle_window_unfocused(window),
            Message::WindowPointerPressed {
                window,
                button,
                status,
            } => self.handle_window_pointer_pressed(window, button, status),
            Message::AddressEditingRequested(pane_id) => self.begin_address_editing(pane_id),
            Message::BreadcrumbSegmentPressed(pane_id, target) => {
                self.activate_breadcrumb_target(pane_id, target)
            }
            Message::AddressDraftChanged(pane_id, value) => {
                self.update_address_draft(pane_id, value)
            }
            Message::AddressEditingSubmitted(pane_id) => self.submit_address_editing(pane_id),
            Message::AddressSuggestionSelected(pane_id, target) => {
                self.submit_address_suggestion(pane_id, target)
            }
            Message::AddressSuggestionInputStabilized(request) => {
                self.load_stable_address_suggestions(request)
            }
            Message::AddressSuggestionsLoaded(request, suggestions) => {
                self.accept_address_suggestions(request, suggestions)
            }
            Message::AddressBarScrolled(pane_id) => Task::batch([
                self.show_scrollbars_temporarily(Region::AddressBar(pane_id)),
                self.request_breadcrumb_drop_target_bounds_measurement(),
            ]),
            Message::SearchInputChanged(value) => self.update_search_input(value),
            Message::SearchSubmitted => self.submit_search(),
            Message::SearchResultsLoaded(generation, outcome) => {
                self.accept_search_results(generation, outcome)
            }
            Message::SearchDirectoryBatchLoaded(generation, hits) => {
                self.accept_directory_search_batch(generation, hits)
            }
            Message::SearchDirectoryFinished(generation, completion) => {
                self.accept_directory_search_finished(generation, completion)
            }
            Message::SearchResultPressed(hit) => self.activate_search_hit(hit),
            Message::SearchCleared => self.clear_search(),
            Message::SearchResultsScrolled => {
                self.show_scrollbars_temporarily(Region::SearchResults)
            }
            Message::SearchServiceEnsured(outcome) => {
                match outcome {
                    Ok(status) => self.search.accept_endpoint_status(status),
                    Err(error) => self.search.accept_endpoint_failure(error),
                }
                Task::none()
            }
            Message::SearchServiceStatusRefreshRequested => search_service_status_command(),
            Message::SearchServiceStatusLoaded(outcome) => {
                match outcome {
                    Ok(status) => self.search.accept_endpoint_status(status),
                    Err(error) => self.search.accept_endpoint_failure(error),
                }
                Task::none()
            }
            Message::SearchServiceRestartRequested => self.restart_search_service(),
            Message::SearchServiceForceRestartPressed => self.press_force_restart_search_service(),
            Message::SearchServiceRecoveryFinished(action, outcome) => {
                self.accept_search_service_recovery(action, outcome)
            }
            Message::SystemThemeDetected(theme) => {
                self.theme = theme;
                Task::none()
            }
            Message::UserPreferencesSaved(result) => self.accept_user_preferences_saved(result),
            Message::AppConfigSaved(result) => self.accept_app_config_saved(result),
            Message::ColumnWidthOverrideSaved(result) => self.accept_column_width_saved(result),
            Message::BrowserSessionSaved(result) => self.accept_browser_session_saved(result),
            Message::BrowserSessionSaveDelayElapsed => {
                self.maybe_flush_pending_browser_session_save()
            }
            Message::ExpandedDirectoryLoadBatch(request, batch) => {
                self.accept_expanded_directory_batch(request, batch)
            }
            Message::ExpandedDirectoryLoaded(request, scan) => {
                self.accept_expanded_directory(request, scan)
            }
            Message::ObservedDirectoryChanged(path) => self.reload_observed_directory(path),
            Message::SettingsOpened => self.open_settings(),
            Message::SettingsCategorySelected(category) => self.select_settings_category(category),
            Message::ApplicationLogsRefreshRequested => self.refresh_application_logs(),
            Message::ApplicationLogsLoaded(request, outcome) => {
                self.accept_application_logs(request, outcome)
            }
            Message::ApplicationLogThresholdSelected(threshold) => {
                self.select_application_log_threshold(threshold)
            }
            Message::ShowHiddenFilesToggled => self.toggle_show_hidden_files(),
            Message::ListDirectorySizeDisplayModeToggled => {
                self.toggle_list_directory_size_display_mode()
            }
            Message::NetworkListThumbnailDownloadsToggled => {
                self.toggle_network_list_thumbnail_downloads()
            }
            Message::SearchContentIndexingToggled => self.toggle_search_content_indexing(),
            Message::MaxPreviewFileMibInputChanged(value) => {
                self.update_max_preview_file_mib_input(value)
            }
            Message::MaxPreviewFileMibInputCommitted => self.commit_max_preview_file_mib_input(),
            Message::LanguageSettingSelected(language_setting) => {
                self.select_language_setting(language_setting)
            }
            Message::StartupLocationPolicySelected(policy) => {
                self.select_startup_location_policy(policy)
            }
            Message::StartupCustomDirectoryInputChanged(value) => {
                self.update_startup_custom_directory_input(value)
            }
            Message::StartupCustomDirectoryCommitted => {
                self.commit_startup_custom_directory_input()
            }
            Message::FileOperationVerificationSelected(verification) => {
                self.user_config.file_operation_verification = verification;
                self.persist_user_preferences_command()
            }
            Message::TerminalEmulatorSelected(terminal_emulator) => {
                self.terminal_emulator = terminal_emulator;
                self.user_config.terminal_emulator = terminal_emulator;
                self.persist_user_preferences_command()
            }
            Message::RenderingGpuPreferenceSelected(preference) => {
                self.select_rendering_gpu_preference(preference)
            }
            Message::RendererRestartRequested => self.restart_with_selected_renderer(),
            Message::RendererRestartNoticeDismissed => self.dismiss_renderer_restart_notice(),
            Message::SmoothScrollWheel(region, delta) => {
                self.handle_smooth_scroll_wheel(region, delta)
            }
            Message::ScrollbarAutoHideElapsed(generation) => {
                self.start_global_scrollbar_hide(generation);
                Task::none()
            }
            Message::WindowChromeAnimationTick => Task::batch([
                self.advance_smooth_scroll_animation(),
                self.advance_scrollbar_animation(),
                self.advance_address_bar_transition(),
                self.advance_tab_bar_reveal_animation(),
                self.advance_tab_animations(),
                self.advance_list_directory_animations(),
                self.advance_sidebar_bookmark_motion(),
            ]),
            Message::SidebarScrolled => self.show_scrollbars_temporarily(Region::Sidebar),
            Message::SettingsScrolled => self.show_scrollbars_temporarily(Region::Settings),
            Message::ShortcutSettingsScrolled => {
                self.show_scrollbars_temporarily(Region::ShortcutSettings)
            }
            Message::PropertiesScrolled => self.show_scrollbars_temporarily(Region::Properties),
            Message::OpenWithApplicationsScrolled => {
                self.show_scrollbars_temporarily(Region::OpenWithApplications)
            }
            Message::OperationQueueScrolled => {
                self.show_scrollbars_temporarily(Region::OperationQueue)
            }
            Message::BatchRenamePreviewScrolled => {
                self.show_scrollbars_temporarily(Region::BatchRenamePreview)
            }
            Message::PreviewDirectoryScrolled => {
                self.show_scrollbars_temporarily(Region::PreviewDirectory)
            }
            Message::PreviewArchiveScrolled => {
                self.show_scrollbars_temporarily(Region::PreviewArchive)
            }
            Message::ColumnBrowserScrolled(pane_id, offset_x, width) => Task::batch([
                self.show_scrollbars_temporarily(Region::ColumnBrowser(pane_id)),
                self.handle_column_browser_scrolled(pane_id, offset_x, width),
            ]),
            Message::ColumnScrolled(pane_id, directory, offset_y, height) => Task::batch([
                self.show_scrollbars_temporarily(Region::Column {
                    pane_id,
                    directory: directory.clone(),
                }),
                self.handle_column_scrolled(pane_id, directory, offset_y, height),
                self.request_browser_session_save(),
            ]),
            Message::ListScrolled(pane_id, offset_y, height) => Task::batch([
                self.show_scrollbars_temporarily(Region::PaneList(pane_id)),
                self.handle_list_scrolled(pane_id, offset_y, height),
                self.request_browser_session_save(),
            ]),
            Message::IconGridScrolled(pane_id, offset_y, width, height) => Task::batch([
                self.show_scrollbars_temporarily(Region::PaneIcons(pane_id)),
                self.handle_icon_grid_scrolled(pane_id, offset_y, width, height),
            ]),
            Message::ColumnResizeStarted(pane_id, column_index) => {
                self.activate_pane(pane_id);
                self.start_column_resize_drag(column_index)
            }
            Message::OpenDirectoryFromMiddleClick(pane_id, path) => {
                self.activate_pane(pane_id);
                Task::batch([
                    self.commit_rename_if_active(),
                    self.open_directory_from_middle_click(path),
                ])
            }
            Message::OpenTrashInNewTab(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.open_trash_in_new_tab()])
            }
            Message::TabPressed(pane_id, tab_id) => {
                self.activate_pane(pane_id);
                let rename_command = self.commit_rename_if_active();
                self.start_tab_drag(pane_id, tab_id);
                Task::batch([rename_command, self.select_tab(tab_id)])
            }
            Message::TabCloseRequested(pane_id, tab_id) => {
                self.activate_pane(pane_id);
                self.close_tab(tab_id)
            }
            Message::TabDragEntered(pane_id, tab_id) => {
                self.reorder_dragged_tab(pane_id, tab_id);
                Task::none()
            }
            Message::TabDragFinished => self.finish_pointer_drag_interactions(),
            Message::NavigateTo(path) => Task::batch([
                self.commit_rename_if_active(),
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]),
            Message::OpenPath(path) => {
                Task::batch([self.commit_rename_if_active(), self.open_path(path)])
            }
            Message::TrashOpened => Task::batch([
                self.commit_rename_if_active(),
                self.open_trash_view(NavigationMode::RecordHistory),
            ]),
            Message::Back => Task::batch([self.commit_rename_if_active(), self.navigate_back()]),
            Message::Forward => {
                Task::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            Message::PaneUp(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_up()])
            }
            Message::PaneBack(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_back()])
            }
            Message::PaneForward(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            Message::AddressInputFocusChecked(pane_id, is_focused) => {
                let checked_session_is_current = self
                    .address_editing
                    .as_ref()
                    .is_some_and(|session| session.pane_id == pane_id);
                if is_focused || !checked_session_is_current {
                    Task::none()
                } else {
                    self.cancel_address_editing()
                }
            }
            Message::RenameInputFocusChecked(is_focused) => {
                if is_focused {
                    Task::none()
                } else {
                    self.commit_rename_if_active()
                }
            }
            Message::RenameInputChanged(value) => self.apply_rename_input_change(value),
            Message::RenameInputUndoRequested => self.undo_rename_input_change(),
            Message::RenameInputRedoRequested => self.redo_rename_input_change(),
            Message::BeginRename(path) => self.begin_rename(path),
            Message::FilePropertiesRequested(path) => self.open_file_properties(path),
            Message::OpenTerminalHere(directory) => self.open_terminal_here(directory),
            Message::RenameSelected => self.commit_rename(),
            Message::CreateDirectory(directory) => self.create_directory_in(directory),
            Message::CreateEmptyFile(directory) => self.create_empty_file_in(directory),
            Message::TrashSelected => self.trash_selected(),
            Message::RestoreSelected => self.restore_selected(),
            Message::EmptyTrashRequested => self.empty_trash_requested(),
            Message::CopySelected => self.copy_selected(),
            Message::MoveSelected => self.move_selected(),
            Message::PastePending => self.paste_pending(),
            Message::FileClipboardWriteFinished(result) => self.accept_file_clipboard_write(result),
            Message::DesktopClipboardReadFinished {
                paste_directory,
                fallback_operation,
                content,
            } => self.accept_desktop_clipboard_paste(paste_directory, fallback_operation, content),
            Message::ClipboardFileCreated(result) => self.accept_clipboard_file_created(result),
            Message::WaylandDndWindowHandleLoaded(handle) => self.accept_wayland_dnd_handle(handle),
            Message::WaylandFilesDropped(result) => self.accept_wayland_file_drop(result),
            Message::FileDropOperationSelected(operation) => {
                self.apply_file_drop_operation(operation)
            }
            Message::FileDropCancelled => self.cancel_file_drop(),
            Message::TransferConflictsChecked {
                mode,
                transfers,
                conflicts,
            } => self.accept_transfer_conflicts_checked(mode, transfers, conflicts),
            Message::TransferConflictChoiceSelected(_)
            | Message::TransferConflictApplyToAllToggled
            | Message::TransferConflictCancelRequested => {
                self.apply_transfer_conflict_message(message)
            }
        }
    }
}
