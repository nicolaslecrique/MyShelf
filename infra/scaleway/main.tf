terraform {
  required_version = ">= 1.10" # use_lockfile below
  required_providers {
    scaleway = {
      source  = "scaleway/scaleway"
      version = "~> 2.92"
    }
  }

  # State lives in a Scaleway Object Storage bucket (S3-compatible), so any
  # machine or CI job with the API key sees the same state. Bucket names are
  # unique across Scaleway, so yours is passed at init (backend.tfbackend).
  # The S3 backend reads AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY: `just tf`
  # fills them from SCW_ACCESS_KEY/SCW_SECRET_KEY.
  backend "s3" {
    key          = "myshelf/terraform.tfstate"
    region       = "fr-par"
    endpoints    = { s3 = "https://s3.fr-par.scw.cloud" }
    use_lockfile = true # lock object next to the state: no concurrent applies

    # Skip checks that only make sense against AWS itself.
    skip_credentials_validation = true
    skip_region_validation      = true
    skip_requesting_account_id  = true
    skip_s3_checksum            = true
  }
}

# Credentials come from the environment, never from files:
#   export SCW_ACCESS_KEY=... SCW_SECRET_KEY=...
provider "scaleway" {
  project_id = var.project_id
  region     = var.region
}

resource "scaleway_container_namespace" "main" {
  name = "myshelf"
}

resource "scaleway_container" "main" {
  name                   = "myshelf"
  namespace_id           = scaleway_container_namespace.main.id
  image                  = "${var.image_repository}:${var.image_tag}"
  port                   = 8080 # matches MYSHELF_LISTEN_ADDRESS in the Dockerfile
  cpu_limit              = 70   # mVCPU; must pair with memory (table in provider docs)
  memory_limit_bytes     = 128 * 1024 * 1024
  min_scale              = 0 # scale to zero when idle
  max_scale              = 1 # caps cost if the public URL gets hammered
  privacy                = "public"
  https_connections_only = true

  environment_variables = {
    # Accept any Host header. Scaleway's gateway already routes by hostname, so
    # only requests for this container's domain ever reach it; the app-level
    # check (DNS-rebinding protection for local servers) adds nothing here.
    MYSHELF_ALLOWED_HOSTS = "[]"
  }

  liveness_probe {
    http {
      path = "/health"
    }
    failure_threshold = 3
    interval          = "30s"
    timeout           = "5s"
  }
}
