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

`just deploy` prints `mcp_url` at the end. The container is **public** (no
authentication): anyone with the URL can call it.
