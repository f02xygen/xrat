use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DnsResolverSettings {
    pub tag: String,
    pub address: String,
    pub path: DnsResolverPath,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DnsResolverPath {
    Direct,
    Proxy,
    Bootstrap,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DnsPolicyRule {
    #[serde(default)]
    pub domain: Vec<String>,
    #[serde(default)]
    pub domain_suffix: Vec<String>,
    pub resolver: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DnsListenerSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
}

impl Default for DnsListenerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 1053,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FakeIpSettings {
    pub enabled: bool,
    pub ipv4_range: String,
    pub ipv6_range: String,
    pub pool_size: u32,
    pub exclude: Vec<String>,
    pub persist: bool,
}

impl Default for FakeIpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            ipv4_range: "198.18.0.0/15".into(),
            ipv6_range: String::new(),
            pool_size: 65535,
            exclude: Vec::new(),
            persist: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DnsOutboundSettings {
    pub tag: String,
    pub rewrite_network: Option<DnsNetwork>,
    pub rewrite_address: Option<String>,
    pub rewrite_port: Option<u16>,
    pub user_level: u32,
    pub rules: Vec<DnsOutboundRule>,
}

impl Default for DnsOutboundSettings {
    fn default() -> Self {
        Self {
            tag: "xrat-dns-out".into(),
            rewrite_network: None,
            rewrite_address: None,
            rewrite_port: None,
            user_level: 0,
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DnsNetwork {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DnsOutboundRule {
    pub action: DnsAction,
    #[serde(default)]
    pub domain: Vec<String>,
    #[serde(default)]
    pub query_type: Vec<u16>,
    #[serde(default)]
    pub response_code: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DnsAction {
    Direct,
    Hijack,
    Drop,
    Return,
}
