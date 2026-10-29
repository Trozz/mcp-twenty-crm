# Combined endpoint

Endpoint `/mcp`. It offers every enabled service's tools and prompts in one MCP server. Today there is one service, `crm`, so `/mcp` offers the same tools as `/crm` with a `crm_` prefix. The `/crm` endpoint keeps working unchanged.

## Tool names

Under `/mcp` every tool is prefixed with its service: `<service>_<tool>`.

| Service | Example names |
|---|---|
| CRM | `crm_search_records`, `crm_list_records`, `crm_create_note`, `crm_delete_record` |

Descriptions, parameters and annotations are the same as on the service's own endpoint, so the [CRM](crm.md) page describes every tool. Only services selected by `TWENTY_SERVICES` appear.

## Prompts

Every service's prompts are offered under `/mcp` with the same prefix as its tools, and their text names the prefixed tools (`crm_search_records` here, `search_records` on `/crm`).

| Prompt | Described on |
|---|---|
| `crm_account_brief` | [CRM](crm.md#prompts) |
| `crm_log_interaction` | [CRM](crm.md#prompts) |

## Safety switches apply

- A read-only service (`TWENTY_READ_ONLY`) is read-only under `/mcp` too: its write tools are hidden and refused.
- Audit lines name the real service and tool, with `"endpoint": "/mcp"`.

## Sign-in

`/mcp` is its own OAuth protected resource (`https://crm-mcp.example.com/mcp`), with metadata at `/.well-known/oauth-protected-resource/mcp`. A token issued for `/mcp` works only there, and a token for `/crm` does not work on `/mcp`. Because `/mcp` is a resource of its own, an OAuth client must name the resource it wants (the `resource` parameter, which claude.ai sends). A Twenty API key sent directly as the bearer token works on every endpoint.

## Turning it off

`/mcp` is served whenever at least one service runs. Set `TWENTY_COMBINED=false` to turn it off; `/mcp` then returns 404.

## Choosing between them

- `/mcp` is what the README and release notes suggest, and keeps working unchanged if more services are added later.
- `/crm` keeps tool names short.
- Do not add both: every tool would appear twice.
