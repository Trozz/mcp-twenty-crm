# mcp-twenty-crm

mcp-twenty-crm is a self-hosted [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for [Twenty](https://twenty.com), the open-source CRM. It lets MCP clients such as claude.ai, Claude Desktop and Claude Code search, read, create and update people, companies, opportunities, notes, tasks and custom objects in a Twenty workspace, on Twenty Cloud or self-hosted.

The server runs one service, `crm`. It has its own MCP endpoint, and a combined endpoint offers it with prefixed tool names:

| Endpoint | Service | What it does |
|---|---|---|
| `/crm` | [CRM](services/crm.md) | 17 tools and 2 prompts: generic record tools that work on every object (including custom ones), the data model, and simple create tools for the standard CRM objects |
| `/mcp` | [Combined](services/combined.md) | Every enabled service's tools and prompts in one connector, named `<service>_<tool>`, for example `crm_search_records` |

## Highlights

- Each person signs in with their own Twenty API key, so one server serves many people and every call runs with that key's role. The server holds no key of its own and stores none.
- A single static Rust binary, also shipped as a `FROM scratch` container image (`ghcr.io/trozz/mcp-twenty-crm`).
- TLS end to end: the server terminates TLS itself, and talks to Twenty over `https` only (plain `http` only to a loopback address).
- A built-in OAuth 2.1 authorization server, so each endpoint works as a claude.ai custom connector. No database and no volume are needed.
- `TWENTY_READ_ONLY=crm` hides and refuses every tool that changes data.
- Every write is recorded in an audit log: tool, outcome and caller, never arguments or results.

## How it talks to Twenty

The server uses Twenty's REST API at `<TWENTY_API_URL>/rest`, sending each person's own API key as the bearer token:

| Purpose | Path |
|---|---|
| Records of any object | `/rest/<object>`, `/rest/<object>/<id>`, `/rest/batch/<object>` |
| The data model | `/rest/metadata/objects` |
| Note and task links | `/rest/noteTargets`, `/rest/taskTargets` |

`TWENTY_API_URL` defaults to Twenty Cloud (`https://api.twenty.com`). For a self-hosted workspace set it to your Twenty server's address.

## What it needs from Twenty

- A recent Twenty release: note and task links use the `targetPerson`, `targetCompany` and `targetOpportunity` fields, and bodies use `bodyV2` Markdown.
- An API key per person, whose role decides what that person can read and change. Listing and describing objects needs a role that may read the data model.
- Twenty allows about 100 API requests per minute per workspace. When the limit is hit, tools say how long to wait.

## Privacy

API keys are never stored on the server: a key is checked once at sign-in, then sealed inside that person's tokens. Logs record tool names, outcomes, durations and a short keyed hash of who called, never record content or keys, unless you explicitly turn on unsafe content logging while debugging. See [Security](security.md).

Built on [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs), the same base as [mcp-icloud](https://github.com/Trozz/mcp-icloud).

Next: [Getting started](getting-started.md).
