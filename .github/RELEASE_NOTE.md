## xrat v0.33.0

This release makes reconnecting easier, keeps TUI runtime indicators current,
and improves keyboard layout support and subscription naming.

### Highlights

- Run `xrat connect` or `xrat connect latest` to reconnect using the most recent
  persisted runtime session. Explicit config IDs and references continue to
  work, including remote targets. If no previous session exists, provide a
  config reference.
- Keep TUI connection and TUN indicators synchronized with changes made by the
  CLI or daemon. Polling recovers from transient errors and uses the shared
  runtime projection, including Windows machine-service TUN state.
- On Unix terminals reporting Kitty base-layout keys, use command shortcuts
  across keyboard layouts while preserving shifted commands and typed Unicode
  text. Legacy text input and standalone Escape remain supported.
- Detect new subscription names from `Profile-Title`, `Content-Disposition`, URL
  fragments, or common query parameters. Explicit names always win, existing
  names survive refresh, and CLI, TUI, and Web imports share the same automatic
  precedence.

### Reliability fixes

- Preserve commands such as `U`, `K`, and `?`, associated Unicode text, and
  AltGr text input. Keep incomplete escape sequences and UTF-8 characters
  buffered until complete; flush ordinary text and lone Escape at the
  appropriate time.
- Preserve subscription names beginning with `sub-` and names chosen by users
  during manual, automatic, and Web refreshes.
- Parse quoted and extended Content-Disposition filenames, including escaped
  quotes, semicolons, language fields, and empty-name fallback.
- Retain dashboard endpoint addresses and service-owned TUN indicators during
  background polling. Regression hardening is included in
  [#279](https://github.com/mhyrzt/xrat/pull/279).

### Contributors

Thank you to the contributors who made these improvements possible:

- [@f02xygen](https://github.com/f02xygen) — reconnecting to the latest config
  in [#274](https://github.com/mhyrzt/xrat/pull/274) and live TUI runtime/TUN
  indicators in [#275](https://github.com/mhyrzt/xrat/pull/275).
- [@XesteRR76](https://github.com/XesteRR76) — layout-independent TUI keyboard
  support in [#276](https://github.com/mhyrzt/xrat/pull/276) and automatic
  subscription naming in [#277](https://github.com/mhyrzt/xrat/pull/277).

### Upgrade notes

- No new database migrations are included. Existing configuration files remain
  valid.
- Windows support remains experimental. Native automated checks do not replace
  the outstanding interactive Windows and actual TUN traffic acceptance work;
  see the [Windows Support roadmap](https://github.com/mhyrzt/xrat/issues/262).
- Refresh an already open browser tab after upgrading.

**Full Changelog**: https://github.com/mhyrzt/xrat/compare/v0.32.0...v0.33.0
