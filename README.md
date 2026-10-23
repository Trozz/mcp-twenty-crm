# mcp-twenty-crm

A self-hosted [Model Context Protocol](https://modelcontextprotocol.io) server for [Twenty](https://twenty.com), the open-source CRM. MCP clients such as claude.ai, Claude Desktop and Claude Code can search, read, create and update people, companies, opportunities, notes, tasks and custom objects in a Twenty workspace, on Twenty Cloud or self-hosted.

| Endpoint | What it does |
|---|---|
| `/crm` | 17 tools and 2 prompts for one Twenty workspace: generic record tools that work on every object (including custom ones), the data model, and simple create tools for the standard CRM objects |
| `/mcp` | Every enabled service's tools and prompts in one connector, named `<service>_<tool>`, for example `crm_search_records` |

- Each person signs in with their own Twenty API key, so one server serves many people and every call runs with that key's role. The server holds no key of its own and stores none.
- A single static Rust binary, also built as a `FROM scratch` container image.
- TLS end to end, and a built-in OAuth 2.1 server so the server works as a claude.ai custom connector. No database.
- `TWENTY_READ_ONLY=crm` hides and refuses every tool that changes data. Every write is recorded in an audit log (tool, outcome and caller; never arguments or results).

Built on [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs), the same base as [mcp-companies-house](https://github.com/Trozz/mcp-companies-house) and [mcp-icloud](https://github.com/Trozz/mcp-icloud).

## Quick start

1. Create an API key in Twenty: **Settings, APIs & Webhooks, Create API key**. Copy it when shown; Twenty does not show it again. The key's role decides what can be read and changed. Everyone who uses the server brings their own key.
2. Build and run the server:

   ```bash
   docker build -t mcp-twenty-crm .
   docker run -d --name mcp-twenty-crm -p 8443:8443 \
     -e PUBLIC_URL=https://crm-mcp.example.com \
     -e OAUTH_SIGNING_KEY="$(openssl rand -hex 32)" \
     -e TWENTY_API_URL=https://crm.example.com \
     -v /path/to/certs:/certs:ro \
     -e TLS_CERT_PATH=/certs/fullchain.pem \
     -e TLS_KEY_PATH=/certs/privkey.pem \
     mcp-twenty-crm
   ```

   Keep `OAUTH_SIGNING_KEY` stable (changing it signs everyone out) and in a secret store. Leave out `TWENTY_API_URL` for Twenty Cloud.
3. Connect a client:
   - **claude.ai, Claude Desktop and mobile**: add a custom connector for `https://crm-mcp.example.com/mcp` and paste your Twenty API key on the sign-in page. claude.ai needs a publicly trusted certificate.
   - **Claude Code**: `claude mcp add --transport http twenty https://crm-mcp.example.com/mcp --header "Authorization: Bearer $TWENTY_API_KEY"`

For local testing without TLS: `PUBLIC_URL=http://localhost:8443 INSECURE_HTTP=true OAUTH_SIGNING_KEY=$(openssl rand -hex 32) cargo run -p mcp-twenty-crm`.

## Configuration

The CRM service reads:

| Variable | Default | Meaning |
|---|---|---|
| `TWENTY_API_URL` | `https://api.twenty.com` | Your Twenty server's address, without `/rest`. https, or http to a loopback address only. Changing it signs everyone out. |
| `TWENTY_DEFAULT_CURRENCY` | `USD` | ISO 4217 code for opportunity amounts given without one, e.g. `GBP`. |
| `TWENTY_SERVICES` | all | Services to run. There is one, `crm`. |
| `TWENTY_READ_ONLY` | none | Services whose write tools are hidden and refused, e.g. `crm`. |
| `TWENTY_COMBINED` | `true` | Also serve every service at `/mcp`. |

The server itself (`PUBLIC_URL`, `OAUTH_SIGNING_KEY`, TLS, ports, logging, limits) is configured as described in the [mcp-server-core configuration reference](https://github.com/Trozz/mcp-server-core-rs). `MCP_AUTH_TOKEN` cannot be used, because everyone signs in with their own key.

## Tools

| Tool | What it does |
|---|---|
| `search_records` | Find records by name across objects (people also by email). Every word must match. |
| `list_records` | Page through any object with a Twenty filter (`name[ilike]:%acme%`, `and(...)`), sort, field selection and cursor paging. |
| `get_record` | Read one record with its related records. |
| `list_objects`, `describe_object` | The workspace's data model: objects, fields, select values and relations. |
| `list_workspace_members` | The workspace's users, for assigning tasks and owning accounts. |
| `list_record_activity` | Notes and tasks linked to a person, company or opportunity. |
| `create_person`, `create_company`, `create_opportunity` | Create from plain fields; the composite fields (name, emails, phones, links, address, amount in micros) are built for you. `extra` takes any other field. |
| `create_note`, `create_task` | Create with a Markdown body and link to people, companies and opportunities in one call. |
| `create_record`, `create_records`, `update_record` | Create one or up to 60 records, or update fields, of any object. |
| `delete_record`, `restore_record` | Move a record to the trash (the default) or destroy it with `permanent: true`; bring it back from the trash. |

Prompts: `account_brief` (a briefing on one company: record, people, opportunities, recent activity) and `log_interaction` (turn a meeting summary into a linked note and follow-up tasks, after you confirm).

Good to know:

- Twenty's own DELETE destroys a record unless told otherwise. `delete_record` asks for a soft delete unless `permanent` is set.
- Note and task links use Twenty's current `targetPerson`, `targetCompany` and `targetOpportunity` fields, and bodies use `bodyV2` Markdown, so the server needs a recent Twenty release.
- Twenty allows about 100 API requests per minute per workspace. When the limit is hit, tools say how long to wait. Reads are retried once after a network error or server error; writes never are.
- Listing objects and describing them needs a key whose role may read the data model.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
```

The tests run against an in-memory fake of the Twenty REST API (`crates/mcp-twenty-crm-records/tests/support/fake_twenty.rs`), including end-to-end MCP sessions through the OAuth and bearer guards.

Layout:

- `crates/mcp-twenty-crm`: the binary, wiring services into `mcp_server_core::app::Server`.
- `crates/mcp-twenty-crm-records`: the `crm` service: REST client (`api.rs`), key check (`verifier.rs`), tools (`tools/`), output shaping (`view.rs`) and prompts.

## License

MIT
