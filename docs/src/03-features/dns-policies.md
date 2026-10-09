# DNS policies and FakeIP

Managed DNS is opt-in. XRAT does not change `/etc/resolv.conf`, system resolver
settings or firewall rules outside the managed TUN lifecycle. Existing
proxy-only configurations keep their current behavior.

## Named resolvers

Use named resolvers to distinguish direct, proxied and bootstrap DNS traffic:

```toml
[dns]
query_strategy = "UseIP"
use_system_hosts = true
bootstrap_resolver = "bootstrap"
final_resolver = "remote"
resolvers = [
  { tag = "bootstrap", address = "udp://1.1.1.1", path = "bootstrap" },
  { tag = "remote", address = "https://dns.google/dns-query", path = "proxy" },
  { tag = "local", address = "udp://192.168.1.1", path = "direct" },
]
rules = [
  { domain = ["router.example"], domain_suffix = ["lan.example"], resolver = "local" },
]

[dns.listener]
enabled = true
host = "127.0.0.1"
port = 1053
```

Remove legacy `dns.servers` entries when migrating to named resolvers. Tags must
be unique; the `xrat-` prefix and managed inbound tags such as `socks-in`,
`tun-in` and `api` belong to generated internal services. Exact and suffix
matchers within a rule mean OR. Rules are ordered before the final resolver.
Explicit hosts and successful system-hosts answers take precedence. Xray rejects
overlapping rules because its DNS server selection cannot preserve the same
first-match contract as sing-box. Sing-box named policies require an explicit
`UseIP`, `UseIPv4` or `UseIPv6` strategy; `UseSystem` has no exact mapping.
Structured resolver/rule arrays are edited in TOML rather than the scalar
settings editor.

The bootstrap resolver must use a literal-IP UDP endpoint and
`path = "bootstrap"`. XRAT uses bounded A/AAAA queries before replacing a
managed session, validates transactions and questions, follows answer-section
CNAME chains, and rejects truncated or unsuccessful responses. TCP fallback for
this bootstrap step is unsupported. Proxy hostname pinning preserves TLS SNI and
transport Host identity. Sing-box uses the bootstrap tag as its explicit domain
resolver. Neither backend falls back to the host resolver for configured named
bootstrap.

Resolver endpoints accept UDP, TCP and HTTPS on both engines, and TLS on
sing-box. Xray rejects DNS-over-TLS. A `+local` spelling is accepted for
migration; the typed `path` controls egress. Endpoint credentials, query
parameters and fragments are rejected. DNS-over-HTTPS keeps its hostname for
certificate verification. DNS failure does not authorize fallback from a proxy
resolver to direct traffic.

The listener serves UDP and TCP on a loopback IP. Choose an unused port and
point applications at it explicitly. Managed TUN additionally intercepts
port 53. Encrypted DNS chosen independently by an application is outside that
interception. Test and scan processes use real resolver policies without TUN
capture or FakeIP allocation.

## Xray DNS outbound

Xray 26.7.11 or newer supports an explicit DNS outbound:

```toml
[dns.outbound]
tag = "xrat-dns-out"
user_level = 0
rules = [
  { action = "return", domain = ["full:blocked.example"], query_type = [1, 28], response_code = 5 },
  { action = "hijack" },
]
```

Rules use Xray domain matcher syntax and numeric DNS record types. `hijack`
answers through the configured internal DNS client; `direct` forwards the DNS
message; `drop` returns no answer; `return` constructs a response with the
chosen rcode (0..15). The default internal outbound remains generated when TUN
or the listener needs it. Explicit configuration replaces that outbound instead
of adding a second competing DNS path.

Optional `rewrite_network = "tcp"` or `"udp"`, `rewrite_address` and
`rewrite_port` control forwarded DNS destinations. They apply to forwarding, not
the internal resolver used by `hijack`. Configured DNS-outbound routing covers
port 53 on managed proxy inbounds as well as the DNS listener. Sing-box rejects
this Xray-specific block and uses its native `hijack-dns` action instead.

## FakeIP

Enable FakeIP only after configuring named real resolvers and either the DNS
listener or TUN:

```toml
[dns.fakeip]
enabled = true
ipv4_range = "198.18.0.0/15"
ipv6_range = ""
pool_size = 65535
exclude = ["router.example"]
persist = false
```

The IPv4 pool must be a network CIDR within `198.18.0.0/15`; an optional IPv6
pool must be a ULA network CIDR. Avoid ranges used by the local network or TUN
interface. DNS caching is required for domain recovery. Xray requires
`query_strategy = "UseIP"`; family-only strategies do not allocate FakeIP.
`pool_size` bounds Xray allocations and must fit the IPv4 CIDR. Sing-box uses
its CIDR capacity and accepts only the default value for this Xray-specific
field.

Explicit hosts retain real answers. Exclusions are exact hostnames: sing-box
uses the final real resolver, while Xray resolves a real bootstrap snapshot
before startup unless a host override exists. Refresh that snapshot by
reconnecting. Xray mappings reset on restart and reject `persist = true`.
Sing-box persistence uses a policy-specific cache with mode `0600` under XRAT's
private runtime directory (`0700` on Unix); changing the resolver, pool, rules
or hosts selects a different cache. Cache files contain DNS mappings and should
remain private to the XRAT user.

Fake addresses work only when connections return through the managed engine.
Applications that use cached fake answers after disconnect cannot reach the
original destination. FakeIP does not provide a persistent kill switch.

V2Ray advanced DNS, encrypted bootstrap, suffix exclusions and system DNS
ownership are explicitly unsupported by this contract.
