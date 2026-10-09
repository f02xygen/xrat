## xrat v0.25.0

This release completes managed DNS policies and strengthens TUN capture and
cleanup for Xray and sing-box.

### Features

- Named DNS resolvers support direct, proxy and explicit bootstrap paths,
  ordered exact/suffix domain rules, and a final resolver. Endpoint/provider
  bootstrap runs before replacing an existing connection.
- Optional loopback DNS listeners accept UDP and TCP. Managed TUN intercepts
  port-53 DNS traffic through the selected engine's DNS policy.
- Xray DNS outbound controls support forwarding, destination rewrites, dropped
  queries and explicit response codes, with preflight validation.
- Opt-in IPv4/IPv6 FakeIP recovers original domains for direct/proxy routing,
  honors fixed hosts and exclusions, and supports private persistent mappings on
  sing-box. Probe and scan flows continue to use real DNS.
- Runtime settings, validation and documentation define supported TUN/DNS
  combinations and reject settings the selected engine cannot represent.

### Fixes

- Xray capture uses split default routes so a physical route's lower metric
  cannot bypass TUN capture.
- Linux interface identity is verified in the active network namespace. Sing-box
  policy rules are recorded with exact selectors and removed safely after a core
  failure, without overwriting foreign interfaces or rules.
- Disconnect, live TUN changes, daemon shutdown, crash recovery and daemon
  SIGINT/SIGTERM cleanup restore owned network state. Failed bootstrap,
  unsupported Xray versions and resource collisions preserve the prior runtime.
- Subscription HTTP errors retain useful status/failure categories while
  removing provider URLs, credentials and response bodies from diagnostics.

### Development and verification

Python helpers are replaced with ordinary Rust integration tests and Justfile
recipes. CI uses checksum-pinned native engines and disposable Linux namespaces
for DNS traffic and TUN lifecycle checks. Local acceptance covered 19 DNS
scenarios and 10 TUN scenarios on Xray 26.7.11/26.9.30 and sing-box 1.13.21;
packet evidence is Linux amd64, with other architectures requiring on-host
verification.

### Upgrade notes

No database migration. New DNS policies and FakeIP are opt-in; existing legacy
DNS server settings remain supported. Restart the daemon after upgrading and
reapply TUN privileges as needed. Xray managed TUN, DNS outbound controls and
FakeIP require a known core version of at least 26.7.11. Sing-box requires at
least 1.13.0 and validates generated configuration with the installed binary.
System DNS settings are not changed by XRAT.

Rust SDK consumers constructing public structs directly must update their
initializers for new generation/DNS fields. Xray DNS server entries now support
structured JSON values. Prefer defaults for unused generation options; see the
DNS policy and managed-capture documentation for supported combinations.

### Thanks

Thank you to [@f02xygen](https://github.com/f02xygen) for the Xray TUN
DNS/bootstrap foundation in [#202](https://github.com/mhyrzt/xrat/pull/202) and
compatibility/diagnostic improvements in
[#206](https://github.com/mhyrzt/xrat/pull/206), which this release builds on.

**Full Changelog**: https://github.com/mhyrzt/xrat/compare/v0.24.0...v0.25.0
