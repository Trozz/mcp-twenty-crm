# Getting started

This page takes you from nothing to a running server. You need a Twenty workspace (Twenty Cloud or self-hosted), and a machine that can run Docker or a single binary.

## 1. Create a Twenty API key

Everyone who uses the server brings their own key. Each person:

1. Opens their Twenty workspace and goes to **Settings**, then **APIs & Webhooks**.
2. Chooses **Create API key** and copies the key when it is shown. Twenty does not show it again.

The key's role decides what can be read and changed through the server. The server itself needs no key: it only checks each person's key when they sign in.

## 2. Choose a secret for the server

The server needs one secret of its own:

- `OAUTH_SIGNING_KEY`: at least 32 bytes, used to sign OAuth tokens and to seal each person's API key inside them. Generate one with `openssl rand -hex 32` and keep it stable: changing it signs everyone out.

Store it in a secret manager or an environment file readable only by you. Every variable can also be read from a file: append `_FILE` to the variable name, for example `OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key`.

There is no admin password and no static token: `MCP_AUTH_TOKEN` cannot be used, because everyone signs in with their own key.

## 3. Run it

### With Docker

```bash
docker run -d --name mcp-twenty-crm -p 8443:8443 \
  -e PUBLIC_URL=https://crm-mcp.example.com \
  -e OAUTH_SIGNING_KEY="$(openssl rand -hex 32)" \
  -e TWENTY_API_URL=https://crm.example.com \
  -v /path/to/certs:/certs:ro \
  -e TLS_CERT_PATH=/certs/fullchain.pem \
  -e TLS_KEY_PATH=/certs/privkey.pem \
  ghcr.io/trozz/mcp-twenty-crm:latest
```

`PUBLIC_URL` is the address clients use to reach the server. `TWENTY_API_URL` is your Twenty server's address, without `/rest`; leave it out for Twenty Cloud. See [TLS and public access](tls.md) for how to make the server reachable with a trusted certificate.

### With Docker Compose and a Cloudflare tunnel

A tunnel gives the server a public HTTPS address with a trusted certificate, without opening a port on your network. Create a tunnel in the Cloudflare dashboard, add a public hostname such as `crm-mcp.example.com` pointing to `https://mcp-twenty-crm:8443` with **No TLS Verify** turned on (the server's own certificate is self-signed in this setup), and copy the tunnel token.

`compose.yaml`:

```yaml
services:
  mcp-twenty-crm:
    image: ghcr.io/trozz/mcp-twenty-crm:latest
    container_name: mcp-twenty-crm
    restart: unless-stopped
    env_file: app.env
    ports:
      - "127.0.0.1:8443:8443"
    volumes:
      - ./audit:/audit
  tunnel:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel --no-autoupdate run
    environment:
      TUNNEL_TOKEN: ${TUNNEL_TOKEN}
    depends_on:
      - mcp-twenty-crm
```

`app.env` (readable only by you, for example `chmod 600 app.env`):

```bash
PUBLIC_URL=https://crm-mcp.example.com
OAUTH_SIGNING_KEY=replace-with-the-output-of-openssl-rand-hex-32
TWENTY_API_URL=https://crm.example.com
TWENTY_DEFAULT_CURRENCY=GBP
AUDIT_LOG_PATH=/audit/audit.jsonl
```

The server runs as UID 65532, so the audit directory must be writable by it: `mkdir -p audit && sudo chown 65532:65532 audit`. Then start both containers with `TUNNEL_TOKEN=<token> docker compose up -d`.

### With the binary

Download the archive for your platform from [Releases](https://github.com/Trozz/mcp-twenty-crm/releases) (Linux x86_64 and arm64, macOS arm64), verify the checksum, extract it and run:

```bash
shasum -a 256 -c mcp-twenty-crm-<version>-<target>.tar.gz.sha256
tar -xzf mcp-twenty-crm-<version>-<target>.tar.gz
cd mcp-twenty-crm-<version>-<target>
PUBLIC_URL=https://crm-mcp.example.com OAUTH_SIGNING_KEY=... \
  TWENTY_API_URL=https://crm.example.com \
  ./mcp-twenty-crm
```

All configuration comes from environment variables; the [configuration reference](configuration.md) lists every one.

### Locally, without TLS

For a quick test from a checkout of the repository:

```bash
PUBLIC_URL=http://localhost:8443 INSECURE_HTTP=true \
  OAUTH_SIGNING_KEY=$(openssl rand -hex 32) LOG_FORMAT=pretty \
  cargo run -p mcp-twenty-crm
```

## 4. Check it is running

```bash
curl -k https://127.0.0.1:8443/healthz
```

prints `ok`. The startup log has a `twenty settings` line with the Twenty origin and default currency, a `service enabled` line for `crm` with its URL and tools, and a `combined endpoint enabled` line for `/mcp`.

`mcp-twenty-crm --version` prints the version, and `mcp-twenty-crm --healthcheck` exits 0 when the local server answers `/healthz` (the container image uses it as its `HEALTHCHECK`).

## 5. Choose what runs

- `TWENTY_READ_ONLY=crm` (or `all`) hides and refuses every tool that changes data.
- `TWENTY_COMBINED=false` turns off the combined `/mcp` endpoint, leaving only `/crm`.
- `TWENTY_DEFAULT_CURRENCY=GBP` sets the currency of opportunity amounts given without one (default `USD`).

Next: [Connecting clients](connecting-clients.md).
