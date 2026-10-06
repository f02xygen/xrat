use serde_json::json;

use xrat_model::Node;

use super::routing::{apply_runtime_routing, field_rule};
use super::tuning::{XrayGenOptions, apply_runtime_tuning};
use super::{Inbound, LogConfig, XrayConfig, outbound::node_to_outbound};

#[cfg(test)]
mod tests;

mod builder;
pub use builder::enable_stats_api;
pub use builder::generate_probe_config;
pub use builder::generate_probe_config_with_options;
pub use builder::generate_runtime_config;
pub use builder::generate_runtime_config_for_inbounds;
pub use builder::generate_runtime_config_for_inbounds_with_options;
pub use builder::generate_runtime_config_with_inbounds;
pub use builder::{XrayTunCaptureOptions, enable_tun_capture};
