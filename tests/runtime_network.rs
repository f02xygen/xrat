#![cfg(target_os = "linux")]
#[path = "runtime_network/dns.rs"]
mod dns;
#[path = "runtime_network/fixtures.rs"]
mod fixtures;
#[path = "runtime_network/process.rs"]
mod process;
#[path = "runtime_network/tun.rs"]
mod tun;

#[test]
#[ignore = "requires Linux namespaces, iproute2, OpenSSL and pinned native engines"]
fn dns_network() {
    if process::enter_namespace("dns_network") {
        dns::exercise();
    }
}
#[test]
#[ignore = "requires Linux namespaces, /dev/net/tun, iproute2 and pinned native engines"]
fn tun_network() {
    if process::enter_namespace("tun_network") {
        tun::exercise();
    }
}
#[test]
#[ignore = "private fixture worker launched by tun_network"]
fn fixture_worker() {
    process::assert_isolated();
    tun::serve();
}
