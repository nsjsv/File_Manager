mod anchored_popup;
mod app;
mod appearance;
mod audio_preview;
mod column_entry_bounds;
mod commands;
mod config;
mod floating_surface;
mod formatting;
mod icons;
mod measured_middle_ellipsized_text;
mod model;
mod operation_history;
mod operation_queue;
mod operation_queue_display;
mod operation_queue_view;
mod preview;
mod selection_marquee;
mod shortcuts;
mod sidebar;
mod startup_index_tree;
mod startup_trace;
mod text_preview;
mod three_column_view;
mod thumbnail_cache;
mod typography;
mod video_preview;
mod view;

struct StartupRenderingVariable {
    key: &'static str,
    value: Option<String>,
}

fn main() -> iced::Result {
    let startup_rendering_variables = startup_rendering_variables();
    restart_with_startup_rendering_environment(&startup_rendering_variables);
    apply_startup_rendering_environment(&startup_rendering_variables);
    startup_trace::init_from_env();
    startup_trace::mark("main_entered");
    app::run()
}

fn startup_rendering_variables() -> Vec<StartupRenderingVariable> {
    let startup_rendering_gpu_preference = config::load_user_config().rendering_gpu_preference;
    let display_renderer_gpu = desktop_linux::detect_display_renderer_gpu();
    let mut variables = vec![StartupRenderingVariable {
        key: "ICED_BACKEND",
        value: Some(
            startup_rendering_gpu_preference
                .iced_backend_candidates()
                .to_owned(),
        ),
    }];

    variables.push(StartupRenderingVariable {
        key: "MESA_VK_DEVICE_SELECT",
        value: startup_rendering_gpu_preference
            .mesa_vulkan_device_select(display_renderer_gpu.as_ref()),
    });
    variables.push(StartupRenderingVariable {
        key: "WGPU_POWER_PREF",
        value: startup_rendering_gpu_preference
            .wgpu_power_preference(display_renderer_gpu.as_ref())
            .map(str::to_owned),
    });

    variables
}

fn restart_with_startup_rendering_environment(variables: &[StartupRenderingVariable]) {
    if startup_rendering_environment_matches(variables) {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let Ok(current_exe) = std::env::current_exe() else {
            return;
        };
        let mut command = std::process::Command::new(current_exe);
        command.args(std::env::args_os().skip(1));
        for variable in variables {
            match &variable.value {
                Some(value) => {
                    command.env(variable.key, value);
                }
                None => {
                    command.env_remove(variable.key);
                }
            }
        }

        // Vulkan 设备选择会被加载器缓存，环境不匹配时必须让变量从进程启动就生效。
        let error = command.exec();
        eprintln!("failed to restart with rendering environment: {error}");
    }
}

fn startup_rendering_environment_matches(variables: &[StartupRenderingVariable]) -> bool {
    variables.iter().all(|variable| match &variable.value {
        Some(value) => std::env::var(variable.key).is_ok_and(|current| current == *value),
        None => std::env::var_os(variable.key).is_none(),
    })
}

fn apply_startup_rendering_environment(variables: &[StartupRenderingVariable]) {
    for variable in variables {
        match &variable.value {
            Some(value) => std::env::set_var(variable.key, value),
            None => std::env::remove_var(variable.key),
        }
    }
}
