# Connecting clients

The CRM service is an MCP server at `/crm`, and `/mcp` offers it (and any future service) with prefixed tool names. Add one connector, for either endpoint.

| Endpoint | Connector URL | Tool names |
|---|---|---|
| CRM | `https://crm-mcp.example.com/crm` | `search_records`, `create_note`, ... |
| All services | `https://crm-mcp.example.com/mcp` | `crm_search_records`, `crm_create_note`, ... |

Use one of them, not both: with both, every tool appears twice.

## claude.ai, Claude Desktop and Claude mobile

These connect from Anthropic's servers, so the server must be reachable from the internet with a publicly trusted certificate (see [TLS and public access](tls.md)).

1. In claude.ai open **Settings**, then **Connectors**, then **Add custom connector**.
2. Enter the connector URL, for example `https://crm-mcp.example.com/mcp`.
3. A sign-in page from your server opens and asks for a **Twenty API key**. Paste your own key (see [Getting started](getting-started.md#1-create-a-twenty-api-key)).

The server checks the key with one request to Twenty, then seals it inside the connector's tokens. Access tokens last one hour and refresh tokens 30 days; the client refreshes its access token itself. Every grant ends 90 days after sign-in, however often it is refreshed, and you sign in again. Connectors added in claude.ai are also available in Claude Desktop and the Claude mobile apps.

Each person in your team adds their own connector and signs in with their own key, so every call runs with that person's role in Twenty.

## Claude Code

Claude Code can send a header, so it can skip OAuth and send your Twenty API key directly as the bearer token:

```bash
claude mcp add --transport http twenty https://crm-mcp.example.com/mcp \
  --header "Authorization: Bearer $TWENTY_API_KEY"
```

or for the CRM endpoint alone:

```bash
claude mcp add --transport http twenty-crm https://crm-mcp.example.com/crm \
  --header "Authorization: Bearer $TWENTY_API_KEY"
```

The server checks a key sent this way with Twenty too, and remembers the verdict (ten minutes for a valid key, one minute for a rejected one) so it does not check on every request. Claude Code also works against a local self-signed certificate if you trust it (see [TLS and public access](tls.md)).

## Other MCP clients

Any client that speaks the MCP Streamable HTTP transport works. It either completes the OAuth flow (dynamic client registration, authorization code with PKCE) or sends `Authorization: Bearer <Twenty API key>`. A request without a token gets a `401` whose `WWW-Authenticate` header points to `/.well-known/oauth-protected-resource/<endpoint>`, where discovery starts.

## Icons

Each endpoint sends its own icon in `serverInfo`, and `/favicon.ico` serves the project icon. Some clients, including claude.ai at the time of writing, show a generic icon for custom connectors regardless.
