# Troubleshooting

## The server does not start

The server prints every configuration problem as `configuration error: ...` and exits. Common ones:

| Message | Fix |
|---|---|
| `PUBLIC_URL is required` | Set `PUBLIC_URL` to the external `https` address, without a path |
| `PUBLIC_URL must not contain a path` | Use the bare origin, for example `https://crm-mcp.example.com` |
| `OAUTH_SIGNING_KEY must be at least 32 bytes` | Use `openssl rand -hex 32` |
| `MCP_AUTH_TOKEN cannot be used when people sign in with their own Twenty API key` | Unset `MCP_AUTH_TOKEN`. Clients that can send a header send their own Twenty key instead; see [Connecting clients](connecting-clients.md#claude-code) |
| `TWENTY_SERVICES contains an unknown service: ... (known: crm)` | Use `crm`, or leave it unset |
| `TWENTY_API_URL must use https (http is only allowed for loopback addresses)` | Use the `https` address of your Twenty server |
| `TWENTY_API_URL is the Twenty server's address without /rest, e.g. https://crm.example.com` | Remove `/rest` from the end |
| `TWENTY_DEFAULT_CURRENCY must be a three-letter ISO 4217 code such as USD, GBP or EUR` | Use a code such as `GBP` |
| `..._FILE: cannot read ...` | The secret file is missing or unreadable by UID 65532 in the container |

## claude.ai cannot connect

- The server must be reachable from the internet with a publicly trusted certificate. A self-signed certificate works only behind a tunnel or proxy; see [TLS and public access](tls.md).
- With a Cloudflare tunnel, the public hostname must point to `https://mcp-twenty-crm:8443` with **No TLS Verify** on.
- `PUBLIC_URL` must be exactly the address claude.ai uses. OAuth metadata and token audiences are built from it.

## The sign-in page refuses the key

- The key must be a whole Twenty API key, copied when Twenty showed it. A value that is not shaped like one is refused without being sent to Twenty.
- The key must belong to the workspace at `TWENTY_API_URL`. Twenty Cloud keys work only with the default `https://api.twenty.com`; a self-hosted key needs your server's address.
- "Twenty's rate limit is used up" or "Twenty did not answer as expected": Twenty was busy or unreachable. Wait and try again; check that the server can reach `TWENTY_API_URL`.
- After several failures from your address, sign-in is blocked for up to a minute.

## A connector stops working with 401

- Rotating `OAUTH_SIGNING_KEY` or changing `TWENTY_API_URL` signs everyone out: remove and add the connector again.
- Every grant ends 90 days after sign-in. Sign in again.
- A token for one endpoint is not accepted on another. Make sure the connector URL is the endpoint it signed in to.

## Tool errors

Tool errors start with a kind, for example `not_found: ...`:

| Kind | Meaning |
|---|---|
| `auth` | Twenty rejected the API key: it expired, was revoked, or belongs to another workspace. Sign in again with a new key |
| `permission` | The key is valid but its role may not read or change this object. Ask a Twenty admin to change the role |
| `not_found` | The object or record id does not exist (any more). Find it again with `search_records` or `list_records` |
| `rate_limited` | Twenty's limit of about 100 requests per minute per workspace is used up. The message says how long to wait |
| `validation` | An argument is wrong, or Twenty rejected the request; the message says why. `describe_object` shows field names and select values |
| `upstream` | Twenty returned an unexpected answer, timed out or could not be reached; usually temporary. "invalid JSON" or "unexpected redirect" means `TWENTY_API_URL` does not point at the Twenty server |
| `internal` | An unexpected error inside the server; check the log and report it if it repeats |
| `read_only` | The service is read-only on this server (`TWENTY_READ_ONLY`) |

## Notes or tasks are not linked

`create_note` and `create_task` need a recent Twenty release with the `targetPerson`, `targetCompany` and `targetOpportunity` fields. On an older release the note or task can be created while its links fail; the result then lists them under `link_errors`. Upgrade Twenty.

## `list_objects` or `describe_object` fails with `permission`

Reading the data model needs a role that may read it. Use a key whose role has that permission, or call the record tools with object names you already know.

## More detail

Set `LOG_LEVEL=info,mcp_twenty_crm_records=debug` or `DEBUG=true` and reproduce the problem. These log request paths, record ids and filters, but not response bodies or API keys. When reporting an issue, include the version (`mcp-twenty-crm --version`) and the log lines, and check that they contain nothing private.
