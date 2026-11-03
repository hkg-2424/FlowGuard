pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::activity_service::{ActivityService, DoctorReport};
pub use application::ports::{ActivitySource, SourceError};
pub use config::FlowGuardConfig;
pub use domain::{
    ActivityCapabilities, ActivityObservation, BrowserContext, FreshnessState, IdleState,
    SourceHealth,
};
pub use infrastructure::activitywatch::ActivityWatchAdapter;
