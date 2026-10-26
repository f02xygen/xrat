mod daemon;
mod enrichment;
mod geo;
mod logs;
mod runtime;

pub use enrichment::enrich_locations;
pub use geo::build_geo_lookup;
pub use logs::{DashboardEvent, DashboardLogs};
pub(crate) use runtime::DashboardRuntimeSnapshot;

use crate::app::context::AppContext;
use crate::app::ports::{Clock, SystemClock};
use crate::app::read_models::ConfigDetail;
use crate::app::runtime_service::{RuntimeService, RuntimeStatusSnapshot};
use std::sync::Arc;
use xrat_db::{ConnectionTestRecord, ConnectionTestRunRecord, SubscriptionRecord};
use xrat_model::ConfigId;

#[cfg(test)]
mod tests;

mod loader;
pub use loader::DaemonOverview;
pub use loader::DashboardService;
pub use loader::DashboardSnapshot;
