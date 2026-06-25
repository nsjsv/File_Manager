use desktop_linux::DisplayRendererGpu;

use crate::config::{self, RenderingGpuPreference, DEFAULT_RENDERING_GPU_PREFERENCE};
use crate::startup_trace;

pub(crate) const ICED_BACKEND_ENV: &str = "ICED_BACKEND";
pub(crate) const MESA_VK_DEVICE_SELECT_ENV: &str = "MESA_VK_DEVICE_SELECT";
pub(crate) const WGPU_POWER_PREF_ENV: &str = "WGPU_POWER_PREF";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupRenderingEnvironment {
    variables: Vec<StartupRenderingVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupRenderingEnvironmentStatus {
    pub(crate) environment: StartupRenderingEnvironment,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupRenderingVariable {
    key: &'static str,
    value: Option<String>,
}

impl StartupRenderingEnvironment {
    pub(crate) fn fast_default() -> Self {
        Self::without_display_probe(DEFAULT_RENDERING_GPU_PREFERENCE)
    }

    pub(crate) fn without_display_probe(preference: RenderingGpuPreference) -> Self {
        Self::from_preference(preference, None)
    }

    pub(crate) fn from_preference(
        preference: RenderingGpuPreference,
        display_renderer_gpu: Option<&DisplayRendererGpu>,
    ) -> Self {
        Self {
            variables: vec![
                StartupRenderingVariable {
                    key: ICED_BACKEND_ENV,
                    value: Some(preference.iced_backend_candidates().to_owned()),
                },
                StartupRenderingVariable {
                    key: MESA_VK_DEVICE_SELECT_ENV,
                    value: preference.mesa_vulkan_device_select(display_renderer_gpu),
                },
                StartupRenderingVariable {
                    key: WGPU_POWER_PREF_ENV,
                    value: preference
                        .wgpu_power_preference(display_renderer_gpu)
                        .map(str::to_owned),
                },
            ],
        }
    }

    pub(crate) fn matches_current_process(&self) -> bool {
        self.variables.iter().all(|variable| match &variable.value {
            Some(value) => std::env::var(variable.key).is_ok_and(|current| current == *value),
            None => std::env::var_os(variable.key).is_none(),
        })
    }

    fn apply_to_current_process(&self) {
        for variable in &self.variables {
            match &variable.value {
                Some(value) => std::env::set_var(variable.key, value),
                None => std::env::remove_var(variable.key),
            }
        }
    }

    fn apply_to_command(&self, command: &mut std::process::Command) {
        for variable in &self.variables {
            match &variable.value {
                Some(value) => {
                    command.env(variable.key, value);
                }
                None => {
                    command.env_remove(variable.key);
                }
            }
        }
    }

    #[cfg(test)]
    fn variable_value(&self, key: &'static str) -> Option<&str> {
        self.variables
            .iter()
            .find(|variable| variable.key == key)
            .and_then(|variable| variable.value.as_deref())
    }
}

impl StartupRenderingEnvironmentStatus {
    pub(crate) fn ready(environment: StartupRenderingEnvironment) -> Self {
        Self {
            environment,
            restart_required: false,
        }
    }

    pub(crate) fn for_loaded_config(preference: RenderingGpuPreference) -> Self {
        let environment = StartupRenderingEnvironment::without_display_probe(preference);
        let restart_required = !environment.matches_current_process();

        Self {
            environment,
            restart_required,
        }
    }
}

pub(crate) fn apply_fast_startup_environment() {
    let preference = config::load_app_config().rendering_gpu_preference;
    let environment = StartupRenderingEnvironment::without_display_probe(preference);
    environment.apply_to_current_process();
    startup_trace::mark("startup_rendering_environment_ready");
}

pub(crate) fn restart_current_process(
    environment: &StartupRenderingEnvironment,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let current_exe = std::env::current_exe()
            .map_err(|error| format!("failed to locate current executable: {error}"))?;
        let mut command = std::process::Command::new(current_exe);
        command.args(std::env::args_os().skip(1));
        environment.apply_to_command(&mut command);

        let error = command.exec();
        Err(format!("failed to restart File Manager: {error}"))
    }

    #[cfg(not(unix))]
    {
        let _ = environment;
        Err("restart is not supported on this platform".to_owned())
    }
}

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
pub(crate) fn environment_matches(
    environment: &StartupRenderingEnvironment,
    current_environment: &HashMap<&'static str, Option<&str>>,
) -> bool {
    environment.variables.iter().all(|variable| {
        let current_value = current_environment
            .get(variable.key)
            .copied()
            .flatten()
            .map(str::to_owned);
        current_value == variable.value
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_linux::DisplayRendererGpuClass;

    #[test]
    fn fast_default_does_not_require_display_gpu_probe_values() {
        let environment = StartupRenderingEnvironment::fast_default();

        assert_eq!(environment.variable_value(ICED_BACKEND_ENV), Some("wgpu"));
        assert_eq!(
            environment.variable_value(WGPU_POWER_PREF_ENV),
            Some("none")
        );
        assert_eq!(environment.variable_value(MESA_VK_DEVICE_SELECT_ENV), None);
    }

    #[test]
    fn display_gpu_environment_uses_detected_gpu_when_available() {
        let gpu = DisplayRendererGpu::from_drm_ids(
            DisplayRendererGpuClass::Integrated,
            "0x1002",
            "0x15bf",
        );

        let environment = StartupRenderingEnvironment::from_preference(
            RenderingGpuPreference::DisplayGpu,
            Some(&gpu),
        );

        assert_eq!(environment.variable_value(ICED_BACKEND_ENV), Some("wgpu"));
        assert_eq!(environment.variable_value(WGPU_POWER_PREF_ENV), Some("low"));
        assert_eq!(
            environment.variable_value(MESA_VK_DEVICE_SELECT_ENV),
            Some("1002:15bf!")
        );
    }

    #[test]
    fn high_performance_environment_avoids_display_gpu_selection() {
        let gpu = DisplayRendererGpu::from_drm_ids(
            DisplayRendererGpuClass::Integrated,
            "0x1002",
            "0x15bf",
        );

        let environment = StartupRenderingEnvironment::from_preference(
            RenderingGpuPreference::HighPerformanceGpu,
            Some(&gpu),
        );

        assert_eq!(environment.variable_value(ICED_BACKEND_ENV), Some("wgpu"));
        assert_eq!(
            environment.variable_value(WGPU_POWER_PREF_ENV),
            Some("high")
        );
        assert_eq!(environment.variable_value(MESA_VK_DEVICE_SELECT_ENV), None);
    }

    #[test]
    fn high_performance_fast_startup_honors_saved_preference_without_probe() {
        let environment = StartupRenderingEnvironment::without_display_probe(
            RenderingGpuPreference::HighPerformanceGpu,
        );

        assert_eq!(environment.variable_value(ICED_BACKEND_ENV), Some("wgpu"));
        assert_eq!(
            environment.variable_value(WGPU_POWER_PREF_ENV),
            Some("high")
        );
        assert_eq!(environment.variable_value(MESA_VK_DEVICE_SELECT_ENV), None);
    }

    #[test]
    fn ready_status_never_requests_restart() {
        let environment = StartupRenderingEnvironment::fast_default();
        let status = StartupRenderingEnvironmentStatus::ready(environment.clone());

        assert_eq!(status.environment, environment);
        assert!(!status.restart_required);
    }

    #[test]
    fn matching_contract_distinguishes_missing_and_different_variables() {
        let environment = StartupRenderingEnvironment::fast_default();
        let mut current_environment = HashMap::new();
        current_environment.insert(ICED_BACKEND_ENV, Some("wgpu"));
        current_environment.insert(WGPU_POWER_PREF_ENV, Some("none"));
        current_environment.insert(MESA_VK_DEVICE_SELECT_ENV, None);

        assert!(environment_matches(&environment, &current_environment));

        current_environment.insert(WGPU_POWER_PREF_ENV, Some("high"));

        assert!(!environment_matches(&environment, &current_environment));
    }
}
