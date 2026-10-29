# Configuration reference

Everything is configured with environment variables. The server checks all of them at startup and refuses to start with a list of every problem it found, so a typo never leaves it half configured.

General rules:

- An empty value counts as unset.
- Every variable can also be read from a file by appending `_FILE` to its name, for example `OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key`. A value set directly wins over the file, and a trailing newline in the file is ignored. Use this for secrets with Docker or Kubernetes secrets.
- Booleans accept `true`, `false`, `1`, `0`, `yes`, `no`, `on` and `off`, in any case.
- Lists are comma separated; spaces around names are ignored.

## Server and HTTP

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `PUBLIC_URL` | yes | | `https://crm-mcp.example.com` | External base URL clients use, without a path. Must be `https`; `http` is accepted only for `localhost` |
| `BIND_ADDR` | no | `0.0.0.0:8443` | `127.0.0.1:8443` | Listen address. It must listen on loopback (`0.0.0.0` does) so `--healthcheck` can reach `127.0.0.1` |
| `TLS_CERT_PATH`, `TLS_KEY_PATH` | no | | `/certs/fullchain.pem`, `/certs/privkey.pem` | PEM certificate chain and private key. Both or neither; reloaded automatically when the files change. See [TLS and public access](tls.md) |
| `INSECURE_HTTP` | no | `false` | `true` | Serve plain HTTP. Only when TLS is terminated in front of the server on a network you trust |
| `ALLOWED_ORIGINS` | no | | `https://app.example.com` | Extra `Origin` values accepted on MCP requests, comma separated. The `PUBLIC_URL` origin is always allowed |
| `TRUSTED_PROXIES` | no | | `10.0.0.2,10.0.0.3` | Proxy IP addresses whose `X-Forwarded-For` is trusted for sign-in rate limiting |

## Services

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `TWENTY_SERVICES` | no | all (`crm`) | `crm` | Services to run. There is one, `crm`. An unknown name or an empty list stops startup |
| `TWENTY_READ_ONLY` | no | none | `crm` or `all` | Services whose write tools are hidden and refused, on their own endpoint and under `/mcp`. An unknown name stops startup. See [Security](security.md#read-only-mode) |
| `TWENTY_COMBINED` | no | `true` | `false` | Serve the [combined endpoint](services/combined.md) `/mcp`. With `false`, `/mcp` returns 404 |

## OAuth

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `OAUTH_SIGNING_KEY` | yes | | output of `openssl rand -hex 32` | Key that signs OAuth tokens and seals each person's Twenty API key inside them, at least 32 bytes. Changing it signs everyone out |

People sign in with their own Twenty API key, so there is no `OAUTH_ADMIN_PASSWORD`, and `MCP_AUTH_TOKEN` must not be set: the server refuses to start with it.

## Twenty

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `TWENTY_API_URL` | no | `https://api.twenty.com` | `https://crm.example.com` | Your Twenty server's address, without `/rest`. Must be `https`, or `http` to a loopback address only; no credentials, query or fragment. Leave it unset for Twenty Cloud. Changing it signs everyone out |
| `TWENTY_DEFAULT_CURRENCY` | no | `USD` | `GBP` | Three-letter ISO 4217 code for opportunity amounts given without one. Upper-cased; anything else stops startup |

## Logging, metrics and audit

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `LOG_LEVEL` | no | `info` | `info,mcp_twenty_crm_records=debug` | Log filter in `tracing` syntax. Targets: `mcp_twenty_crm`, `mcp_twenty_crm_records`, `mcp_server_core`. The `audit` target always stays on at `info` |
| `LOG_FORMAT` | no | `json` | `pretty` | `json` or `pretty` |
| `DEBUG` | no | `false` | `true` | Debug logging for `mcp_twenty_crm*` and `mcp_server_core`: the arguments of read tools (search words, filters), Twenty request paths with record ids, status codes, timings and tool error details. Never API keys or response bodies |
| `DEBUG_UNSAFE_LOG_CONTENT` | no | `false` | `true` | Also logs full tool results and Twenty response bodies, which **puts your CRM records in the logs**. Never in production. API keys are never logged |
| `METRICS_ADDR` | no | | `0.0.0.0:9090` | Starts a separate plain-HTTP Prometheus listener. Keep it on an internal network |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | | `https://otel.example.com` | Exports traces over OTLP/HTTP. The standard `OTEL_EXPORTER_OTLP_HEADERS` is honoured. Nothing is exported unless it is set |
| `AUDIT_LOG_PATH` | no | | `/audit/audit.jsonl` | File every write tool call is appended to as one JSON line (created with mode 0600). Audit lines are always logged too. See [Security](security.md#audit-log) |

## A complete example

```bash
PUBLIC_URL=https://crm-mcp.example.com
OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key
TWENTY_API_URL=https://crm.example.com
TWENTY_DEFAULT_CURRENCY=GBP
TLS_CERT_PATH=/certs/fullchain.pem
TLS_KEY_PATH=/certs/privkey.pem
AUDIT_LOG_PATH=/audit/audit.jsonl
LOG_FORMAT=json
```
