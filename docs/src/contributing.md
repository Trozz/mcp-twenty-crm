# Contributing

## Build and test

You need Rust 1.99.1 (the workspace pins it in `rust-toolchain.toml`).

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
bash scripts/test-next-version.sh
```

The tests run against an in-memory fake of the Twenty REST API (`crates/mcp-twenty-crm-records/tests/support/fake_twenty.rs`), including end-to-end MCP sessions through the OAuth and bearer guards, so they need no Twenty workspace or API key.

To try the server against a real workspace, run it locally (see [Getting started](getting-started.md#locally-without-tls)) with `TWENTY_API_URL` pointing at a test workspace, and connect a client with an API key from that workspace.

## This documentation

The site is built with [mdBook](https://rust-lang.github.io/mdBook/) from `docs/`. To preview it:

```bash
cargo install --locked mdbook@0.5.4 mdbook-mermaid@0.17.1 lychee@0.24.2
bash scripts/build-docs.sh
mdbook serve docs --open
```

`scripts/build-docs.sh` builds the book and checks every internal link and anchor. When you add or change a tool, prompt or environment variable, update the [CRM](services/crm.md) page or the [configuration reference](configuration.md) in the same pull request.

## Adding a service

Each service is a library crate that implements the `Service` trait from `mcp_server_core::service`:

1. Create `crates/mcp-twenty-crm-<name>` as a library. Read its own `TWENTY_` variables in `pub fn from_env(env: &mut ServiceEnv) -> Option<Arc<dyn Service>>`, reporting problems through `env.errors`. Never read the server variables.
2. Build the MCP endpoint in the trait's `router()` with `mcp_server_core::service::mcp_service`, and return the same MCP server from `handler()` so the combined endpoint can offer it. Service names are lowercase path segments without `_` (the combined endpoint uses `_` to separate the service from the tool name), and `mcp` is reserved.
3. Only one enabled service may define how people sign in (`credential_verifier()`); today that is `crm`.
4. Add the crate to `crates/mcp-twenty-crm/Cargo.toml` and one line to `SERVICES` in `crates/mcp-twenty-crm/src/lib.rs`.
5. Document its tools on a new page under `docs/src/services/` and its variables in `docs/src/configuration.md`.

Routing, OAuth metadata, token audiences, `TWENTY_SERVICES`, read-only mode, audit, the combined endpoint and the release pipeline pick the new service up without further changes.

## Pull requests

- Branch from `develop` as `feature/<name>`, `bugfix/<name>` or `hotfix/<name>`, and open the pull request against `develop`.
- Use conventional commit messages (`feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`).
- Test data must be synthetic: never copy real CRM records or API keys into fixtures.
- CI runs fmt, clippy, tests, cargo-deny, a Docker build, the script checks and the documentation build.
