output "mcp_url" {
  value       = "${scaleway_container.main.public_endpoint}/mcp"
  description = "Public MCP endpoint."
}
