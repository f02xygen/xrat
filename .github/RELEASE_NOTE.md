## xrat v0.24.0

This release adds DNS interception and bootstrap resolution for Xray TUN,
improves TLS link compatibility, and preserves useful proxy-test errors.

### Features

- Xray TUN intercepts TCP/UDP port 53 traffic through a dedicated DNS outbound.
  If no DNS servers are configured, it uses Cloudflare and Google DoH endpoints
  over IP.
- Proxy endpoint and DNS provider domains are resolved before TUN startup to
  prevent bootstrap routing loops. TLS/Reality server names are preserved when
  the proxy endpoint is replaced with its resolved IP.
- Private and local network ranges receive direct routing, with a matching
  direct outbound when needed.

### Fixes

- DNS bootstrap parsing handles IPv6 literals, bracketed IPv6 URLs, optional
  ports, and Xray DNS schemes including `https+local`, `tcp+local`,
  `quic+local`, and `h2c`. IP literals and special `localhost`/`fakedns` values
  do not trigger hostname resolution.
- Failed bootstrap resolution aborts before replacing an existing runtime,
  preserving its process and persisted session state.
- VLESS/TLS links accept `cs`/`cipherSuites`, emitting Xray `cipherSuites` and
  sing-box `cipher_suites`.
- HTTP clients send `User-Agent: xrat/0.24.0` by default, while the configurable
  async client preserves custom agents. HTTP failures include their underlying
  details.
- Successful proxy probes supersede ICMP failures. Probe error selection follows
  overall-status priority, so custom test ordering cannot erase or replace a
  real proxy failure with a later ping result.

### Upgrade notes

No CLI flags or database migrations changed. Restart the daemon after upgrading
to load the new executable, and reapply TUN privileges as needed. Xray TUN still
requires a supported Xray core and the existing platform permissions.

Xray TUN now pre-resolves endpoint/provider domains and fails early if bootstrap
resolution is unavailable. Static mappings last for that runtime launch;
reconnect to refresh them. Custom DNS transports keep their existing Xray
behavior, including direct/system resolution for local modes.

Rust SDK consumers constructing public structs directly must account for new
fields: `TlsSettings.cipher_suites`, `RoutingRule.port`/`network`, and
`RuntimeProcessPorts.resolver`. Use `None` for unused optional fields and the
default system resolver (or an injected `HostResolver`) for runtime process
ports.

### Thanks

Thank you to [@f02xygen](https://github.com/f02xygen) for contributing Xray TUN
DNS interception/bootstrap in [#202](https://github.com/mhyrzt/xrat/pull/202)
and the TLS, HTTP, and probe improvements in
[#206](https://github.com/mhyrzt/xrat/pull/206).

**Full Changelog**: https://github.com/mhyrzt/xrat/compare/v0.23.1...v0.24.0
