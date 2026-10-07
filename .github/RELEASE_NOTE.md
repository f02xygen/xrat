## xrat v0.23.1

This patch release makes failed GitHub release requests actionable.

### Fixes

- `xrat upgrade` identifies whether release lookup, archive download, or checksum download failed instead of reporting only `HTTP request failed`.
- HTTP errors retain their underlying causes, including connection and TLS details, and release errors suggest retrying through a working proxy.
- Upgrade documentation includes a SOCKS proxy example and explains the difference between selecting a release version and building from a local checkout.

### Upgrade notes

This improves diagnostics; it does not repair blocked or unavailable GitHub connectivity. The fix is available after upgrading to this version. If release downloads are unreachable, use a working proxy or `xrat upgrade --source` from a local checkout (Cargo may still need missing dependencies).

No CLI flags or database migrations changed. TUN behavior remains as documented for v0.23.0; after replacing the binary, reapply TUN privileges as needed and restart the running daemon to load the new executable.

**Full Changelog**: https://github.com/mhyrzt/xrat/compare/v0.23.0...v0.23.1
