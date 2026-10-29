# Operations

## Health

- `GET /healthz` returns `ok` without authentication.
- `mcp-twenty-crm --healthcheck` connects to `127.0.0.1` on the `BIND_ADDR` port and exits 0 when `/healthz` answers. The container image uses it as its `HEALTHCHECK`, so `docker ps` shows `healthy`.
- `/healthz` does not contact Twenty, so a healthy server can still fail tool calls when Twenty is down.

## Logs

- Logs go to stdout, as JSON by default (`LOG_FORMAT=pretty` for humans).
- Every tool call logs a `tool call` line with the tool, outcome and duration, and a failed call logs `tool failed` with its error kind. Startup logs `twenty settings` (the Twenty origin and default currency), one `service enabled` line with the URL and tools, and `combined endpoint enabled`.
- `LOG_LEVEL` takes `tracing` filters, for example `info,mcp_twenty_crm_records=debug`. The MCP library targets that log session ids are capped at `warn` whatever you set.
- `DEBUG=true` adds read tool arguments, each Twenty request (method, path, status and duration) and tool error details, without response bodies.
- `DEBUG_UNSAFE_LOG_CONTENT=true` adds tool results and Twenty response bodies. Use it only briefly; see [Security](security.md#what-is-never-logged).

## Metrics

`METRICS_ADDR` (for example `0.0.0.0:9090`) starts a separate plain-HTTP Prometheus listener. Keep it on an internal network. Metrics:

| Metric | What it counts |
|---|---|
| `mcp_tool_calls_total`, `mcp_tool_duration_seconds` | Tool calls by tool and outcome, and their duration |
| `twenty_requests_total`, `twenty_request_duration_seconds` | Requests to Twenty by endpoint label and status class, and their duration |
| `twenty_rate_limited_total` | Answers from Twenty with HTTP 429 |
| `oauth_requests_total`, `oauth_login_failures_total` | OAuth requests by endpoint and outcome, and failed sign-ins |
| `bearer_credential_checks_total` | Checks of API keys sent as bearer tokens, by outcome |
| `http_requests_total` | HTTP requests by route and status |

## Traces

`OTEL_EXPORTER_OTLP_ENDPOINT`, with the standard `OTEL_EXPORTER_OTLP_HEADERS` when your collector needs them, exports traces over OTLP/HTTP. Nothing is exported unless it is set.

## Audit file

With `AUDIT_LOG_PATH`, the audit file only grows: one short line per write. Rotate it with a tool that truncates in place, such as logrotate with `copytruncate`. See [Security](security.md#audit-log) for the line format.

## Upgrading

- Docker: pull the new image and recreate the container (`docker compose pull && docker compose up -d`). The container is stateless.
- Binary: replace the file and restart.
- Connectors stay signed in across upgrades and restarts as long as `OAUTH_SIGNING_KEY` and `TWENTY_API_URL` stay the same.
- Twenty upgrades need nothing on this side, as long as Twenty keeps the REST API this server uses; see [Introduction](introduction.md#what-it-needs-from-twenty).
- Release notes list every change; see [Releases](releases.md).

## Scaling and replicas

Everything that matters lives in tokens, so several replicas need only share `OAUTH_SIGNING_KEY` and the same configuration. Sign-in limits and one-time codes are kept per replica. On `SIGTERM` the server drains open connections for up to ten seconds.

## Backups

There is nothing to back up: Twenty holds all data, and the server keeps nothing on disk apart from the optional audit file. Keep your configuration and `OAUTH_SIGNING_KEY` somewhere safe.

## Resource use

The server holds no cache of CRM data; memory is used by open MCP sessions and in-flight requests. The container needs no writable filesystem apart from the audit volume.
