variable "project_id" {
  type        = string
  description = "Scaleway project id to deploy into."
}

variable "region" {
  type    = string
  default = "fr-par"
}

variable "image_repository" {
  type        = string
  default     = "ghcr.io/nicolaslecrique/myshelf"
  description = "Public image repository, published by CI on every green push to main. Point it at your fork's package to deploy your own build."
}

variable "image_tag" {
  type        = string
  description = "Image tag to deploy: a full commit sha CI has published (`just deploy` passes the current HEAD)."
}

# Login for /mcp. Both empty (the default): /mcp is not served, only
# /public/mcp (no login).
variable "auth_issuer" {
  type        = string
  default     = ""
  description = "Issuer URL of the OAuth / OpenID Connect provider users log in with, exactly as in its tokens' `iss` claim (e.g. your WorkOS AuthKit domain, https://<name>.authkit.app)."
}

variable "auth_resource" {
  type        = string
  default     = ""
  description = "Public URL of /mcp (the `mcp_url` output, or your custom domain + /mcp), registered with the provider as the tokens' audience."

  validation {
    condition     = (var.auth_issuer == "") == (var.auth_resource == "")
    error_message = "Set both auth_issuer and auth_resource, or neither."
  }
}
