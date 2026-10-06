//! Typed Xray configuration generation. Generating a config does not launch Xray.

pub use xrat_config::parsing::core::{
    ApiObject, LevelPolicyObject, PolicyObject, SystemPolicyObject,
};
pub use xrat_engines::xray::config::{
    FragmentOptions, GrpcSettings, HttpUpgradeSettings, Inbound, KcpSettings, LogConfig, Mux,
    MuxOptions, Outbound, RawSettings, RealitySettings, RoutingConfig, RoutingRule, Sockopt,
    StreamSettings, TlsSettings, WsSettings, XhttpSettings, XrayCompatibilityPolicy,
    XrayCompatibilityTarget, XrayConfig, XrayDnsConfig, XrayDnsHostValue, XrayGenOptions,
    XrayRouteList, XrayRoutingOptions, XrayTunCaptureOptions, enable_stats_api, enable_tun_capture,
    generate_probe_config, generate_probe_config_with_options, generate_runtime_config,
    generate_runtime_config_for_inbounds, generate_runtime_config_for_inbounds_with_options,
    generate_runtime_config_with_inbounds,
};
