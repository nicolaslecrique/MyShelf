output "mcp_url" {
  value       = "${scaleway_container.main.public_endpoint}/mcp"
  description = "MCP endpoint for logged-in users (served only when auth_issuer is set)."
}

output "public_mcp_url" {
  value       = "${scaleway_container.main.public_endpoint}/public/mcp"
  description = "MCP endpoint that needs no login (public tools only)."
}
