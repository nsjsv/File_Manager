// 当前 finish phase 只收口搜索移除遗留和验证门禁，不在这里为历史样式问题做大面积无关重构。
#![allow(
    clippy::clone_on_copy,
    clippy::cloned_ref_to_slice_refs,
    clippy::collapsible_if,
    clippy::derivable_impls,
    clippy::field_reassign_with_default,
    clippy::filter_map_bool_then,
    clippy::let_and_return,
    clippy::manual_clamp,
    clippy::manual_is_multiple_of,
    clippy::needless_option_as_deref,
    clippy::needless_return,
    clippy::nonminimal_bool,
    clippy::ptr_arg,
    clippy::redundant_pattern_matching,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_map_or,
    clippy::unnecessary_sort_by,
    clippy::unwrap_or_default,
    clippy::useless_conversion
)]

mod anchored_popup;
mod animated_image_preview;
mod app;
mod appearance;
mod audio_preview;
mod column_entry_bounds;
mod commands;
mod config;
mod directory_summary;
mod file_entry_presentation;
mod file_entry_view;
mod floating_surface;
mod formatting;
mod icons;
mod input_blocking_space;
mod list_view;
mod localization;
mod measured_middle_ellipsized_text;
mod model;
mod network_connections;
mod network_preview_cache;
mod open_with;
mod operation_history;
mod operation_queue;
mod operation_queue_display;
mod operation_queue_view;
mod preview;
mod runtime_logging;
mod selection_marquee;
mod shortcuts;
mod sidebar;
mod sidebar_devices;
mod startup_rendering;
mod startup_trace;
mod text_preview;
mod text_preview_loading;
mod text_preview_viewer;
mod three_column_view;
mod thumbnail_cache;
mod typography;
mod video_preview;
mod view;
mod virtual_range;
mod visible_entries;

fn main() -> iced::Result {
    runtime_logging::init_from_env();
    startup_trace::init_from_env();
    startup_trace::mark("main_entered");
    startup_rendering::apply_fast_startup_environment();
    app::run()
}
