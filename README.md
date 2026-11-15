# MyShelf
## Deploy your own (Scaleway Serverless Containers)

CI publishes a public image to `ghcr.io/nicolaslecrique/myshelf`, tagged with
the commit sha, on every green push to `main`. Deploying needs only Terraform
(or OpenTofu), `just`, and a Scaleway API key
(`export SCW_ACCESS_KEY=... SCW_SECRET_KEY=...`).

```sh
cp infra/scaleway/terraform.tfvars.example infra/scaleway/terraform.tfvars  # set project_id
terraform -chdir=infra/scaleway init
just deploy   # deploys the image built for the current commit (HEAD)
```

`just deploy` prints `mcp_url` at the end. The container is **public** (no
authentication): anyone with the URL can call it.
