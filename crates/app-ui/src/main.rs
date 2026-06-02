mod anchored_popup;
mod app;
mod appearance;
mod audio_preview;
mod commands;
mod config;
mod floating_surface;
mod formatting;
mod icons;
mod model;
mod operation_queue;
mod operation_queue_view;
mod preview;
mod selection_marquee;
mod sidebar;
mod startup_trace;
mod text_preview;
mod three_column_view;
mod thumbnail_cache;
mod typography;
mod video_preview;
mod view;

fn main() -> iced::Result {
    startup_trace::init_from_env();
    startup_trace::mark("main_entered");
    app::run()
}
