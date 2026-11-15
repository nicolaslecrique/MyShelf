terraform {
  required_version = ">= 1.5"
  required_providers {
    scaleway = {
      source  = "scaleway/scaleway"
      version = "~> 2.92"
    }
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
