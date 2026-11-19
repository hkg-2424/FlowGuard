pub mod activitywatch;
pub mod auto_source;
pub mod native_windows;
pub mod persistence;

pub use auto_source::AutoSourceAdapter;
pub use native_windows::NativeWindowsAdapter;

use crate::application::ports::ActivitySource;
use crate::config::FlowGuardConfig;

/// Instantiates an activity observation source based on user configuration.
///
/// Supported providers:
/// - `"auto"` or `"activitywatch"`: Uses `AutoSourceAdapter`, trying ActivityWatch first
///   and seamlessly falling back to `NativeWindowsAdapter` if ActivityWatch is unreachable or stopped.
/// - `"native_windows"` / `"native"` / `"windows"`: Exclusively uses `NativeWindowsAdapter`
///   directly via Win32 APIs with zero external dependencies.
/// - `"activitywatch_strict"`: Strictly queries ActivityWatch without native fallback.
pub fn create_activity_source(config: &FlowGuardConfig) -> Box<dyn ActivitySource> {
    let provider = config.activity_source.provider.to_lowercase();
    match provider.as_str() {
        "native_windows" | "native" | "windows" => {
            let afk_timeout = config.activity_source.activitywatch.afk_timeout_secs;
            Box::new(NativeWindowsAdapter::new(afk_timeout))
        }
        "activitywatch_strict" => Box::new(activitywatch::ActivityWatchAdapter::new(
            config.activity_source.activitywatch.clone(),
        )),
        _ => {
            // Default ("auto", "activitywatch", or unspecified):
            // AutoSourceAdapter tries ActivityWatch and seamlessly falls back to Native Windows
            Box::new(AutoSourceAdapter::new(
                config.activity_source.activitywatch.clone(),
            ))
        }
    }
}
