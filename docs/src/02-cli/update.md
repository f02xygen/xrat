# update

Refresh stored subscriptions.

```bash
xrat update [SUBS_REF...]
```

If no refs are provided, xrat refreshes all subscriptions with stored source
values. If refs are provided, xrat refreshes only matching subscriptions
(numeric IDs and stable ref prefixes are both accepted).

## Examples

Refresh all subscriptions:

```bash
xrat update
```

Refresh selected subscriptions:

```bash
xrat update 7 feedbeef
```

## Failure diagnostics

Updates retain per-subscription success/failure counts. Failures distinguish
request timeouts, DNS lookup or connection failures, TLS verification,
redirect limits, unreadable responses, and HTTP status codes such as 401, 404,
429 or 503. Unknown transport failures remain explicitly unknown.

Subscription error messages omit URLs, credentials, path/query tokens, redirect
targets and response bodies. Status descriptions come from the standard HTTP
status code rather than an untrusted server response. These diagnostics are shared
by CLI and TUI updates.
