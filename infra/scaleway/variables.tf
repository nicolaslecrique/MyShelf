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
