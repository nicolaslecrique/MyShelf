# MyShelf
## Deploy your own (Scaleway Serverless Containers)

CI publishes a public image to `ghcr.io/nicolaslecrique/myshelf`, tagged with
the commit sha, on every green push to `main`. Deploying needs only Terraform
(or OpenTofu) 1.10+, `just`, and a Scaleway API key
(`export SCW_ACCESS_KEY=... SCW_SECRET_KEY=...`).

Terraform state is kept in a Scaleway Object Storage bucket, not on your
machine, so any checkout (or CI job) with the key deploys against the same
state. Once, create that bucket in the Scaleway console: Object Storage, region
`fr-par`, any free name, **versioning enabled** (lets you restore an earlier
state). Then:

```sh
cp infra/scaleway/backend.tfbackend.example infra/scaleway/backend.tfbackend    # set bucket
cp infra/scaleway/terraform.tfvars.example infra/scaleway/terraform.tfvars      # set project_id
just tf-init   # once per checkout
just deploy    # deploys the image built for the current commit (HEAD)
```

Without the two files (e.g. in CI), pass the same values directly:
`TF_VAR_project_id=... just tf init -backend-config=bucket=...`.

`just deploy` prints `mcp_url` (needs login, served once login is configured,
see [Login](#login)) and `public_mcp_url` (no login, public tools only).

### Deploy from GitHub Actions

The **Deploy** workflow is manual only: Actions > Deploy > Run workflow, then
enter the commit sha of the image to deploy (leave it empty to deploy the head
of the branch picked in "Use workflow from"). It checks the image exists on
ghcr.io before running `terraform apply`. Once, in Settings > Environments,
create a `production` environment with:

- secrets `SCW_ACCESS_KEY` and `SCW_SECRET_KEY`
- variables `SCW_PROJECT_ID` and `TF_STATE_BUCKET`, and optionally
  `AUTH_ISSUER` and `AUTH_RESOURCE` (see [Login](#login))
- optionally, required reviewers, so every run waits for an approval

## Login

MyShelf serves MCP at two endpoints:

- `/mcp`: every tool, for logged-in users only. Served only when login is
  configured.
- `/public/mcp`: no login, public tools only (`hello_world`).

Login follows the MCP authorization spec (OAuth 2.1), which Claude, ChatGPT, Le
Chat and Gemini all implement. MyShelf never handles passwords: an identity
provider logs users in and gives the AI app an access token, which MyShelf only
checks. A request to `/mcp` without a token gets a 401 pointing at
`/.well-known/oauth-protected-resource/mcp`, which names the provider; that is
how apps find where to log in.

The code is not tied to a provider. Any OAuth / OpenID Connect provider works if
it:

- issues **JWT** access tokens (not opaque ones), signed with an asymmetric key
  (RS256, ES256...) whose id is in the token header (`kid`),
- puts the `/mcp` URL in the tokens' audience (`aud`), from the `resource`
  parameter the apps send (RFC 8707 resource indicators),
- lets AI apps register themselves: Dynamic Client Registration and/or Client
  ID Metadata Documents, with Authorization Code + PKCE,
- publishes its metadata, including `jwks_uri` (where its signing keys are),
  at `<issuer>/.well-known/oauth-authorization-server` or
  `<issuer>/.well-known/openid-configuration`.

Users are identified by their provider (issuer) and their id there (`sub`):
switching provider, or changing the issuer URL, gives every user a new identity.
Pick the final issuer URL (e.g. a custom domain) before having users.

Configuration: `[auth]` in `myshelf.toml` (see `myshelf.example.toml`) or the
`MYSHELF_AUTH__*` env vars; on Scaleway, the `auth_issuer` and `auth_resource`
Terraform variables.

### With WorkOS AuthKit

1. Deploy once without login to get `mcp_url`.
2. In the WorkOS dashboard:
   - enable **Dynamic Client Registration** and **Client ID Metadata
     Document** (AuthKit's MCP settings), so AI apps can register themselves,
   - add `mcp_url` as a **resource indicator** and make it the default. Without
     it, tokens carry an environment-wide audience and MyShelf refuses them,
   - optionally set a custom auth domain (do it before having users, see
     above), and choose the sign-in methods.
3. Set `auth_issuer` (your AuthKit domain, e.g. `https://<name>.authkit.app`)
   and `auth_resource` (`mcp_url`) in `terraform.tfvars`, or as the
   `AUTH_ISSUER` / `AUTH_RESOURCE` variables of the GitHub `production`
   environment, then deploy again.
4. Check: `curl -i -X POST <mcp_url>` answers 401 with a `WWW-Authenticate`
   header, and `<origin>/.well-known/oauth-protected-resource/mcp` names your
   AuthKit domain.

Then add `mcp_url` as a custom connector (or app) in Claude, ChatGPT, Le Chat
or Gemini, on the web or desktop app: it asks the user to log in once, then
works on the mobile apps too.

## Logs

The server logs to stdout, one JSON object per line: one at startup, one per
HTTP request (method, path, status, duration) and one at shutdown. Pipe through
`jq` to read them locally (`just run | jq`). `log_filter` (`MYSHELF_LOG_FILTER`, see
`myshelf.example.toml`) picks levels, e.g. `debug,rmcp=warn`.

On Scaleway, Cockpit collects stdout: query the container's logs from
Cockpit's Grafana (Explore), adding `| json` to filter on fields, e.g.
`| json | status >= 500`. Logs kept for the default 7 days are free.

## Architecture

| File | Role |
|---|---|
| `src/main.rs` | Wires everything: HTTP routes, `/mcp` behind login, `/public/mcp`, graceful shutdown |
| `src/config.rs` | Configuration (defaults, `myshelf.toml`, `MYSHELF_*` env vars) |
| `src/auth.rs` | Checks OAuth access tokens and serves the protected resource metadata |
| `src/my_shelf.rs` | The MCP tools |
| `src/logging.rs` | JSON logs to stdout |

**Rule: `auth.rs` does not depend on the rest of the app.** It never imports
from `crate::` (no `Config`, no `MyShelf`) and has no built-in values (URLs,
durations, names): everything comes from `AuthSettings`, which `main.rs` builds
from the config. Only external crates (axum, rmcp, jsonwebtoken, reqwest...)
are allowed, so it could be moved to its own crate and used by another app.
Nothing checks this automatically yet: keep it in mind when changing it.
