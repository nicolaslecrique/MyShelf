// The rmcp macros generate async trait methods without `.await`.
#![allow(clippy::unused_async_trait_impl)]

use std::sync::Arc;

use rmcp::{
    tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

#[derive(Clone)]
struct MyShelf;

#[tool_router(server_handler)]
impl MyShelf {
    /// Say hello from `MyShelf`.
    #[tool]
    #[allow(clippy::unused_self)]
    fn hello_world(&self) -> String {
        "Hello, world! This is MyShelf.".to_string()
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mcp = StreamableHttpService::new(
        || Ok(MyShelf),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new().nest_service("/mcp", mcp);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("MyShelf MCP server listening on http://127.0.0.1:3000/mcp");
    axum::serve(listener, app).await
}
