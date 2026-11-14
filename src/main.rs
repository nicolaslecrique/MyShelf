// BACKGROUND: MCP (Model Context Protocol) is a protocol that lets an AI client
// (e.g. Claude) discover and call "tools" exposed by a server. This file is such
// a server. It exposes one tool, `hello_world`, over HTTP at /mcp.

// `#![...]` (with the `!`) is an *inner attribute*: it applies to the whole file/crate.
// `allow(...)` silences a lint (a compiler/clippy warning). This project sets clippy
// `pedantic` to `deny` in Cargo.toml (warnings become errors), and the rmcp macros
// below generate `async` code with no `.await` inside, which would trip this lint.
// We can't edit generated code, so we silence the lint here.
#![allow(clippy::unused_async_trait_impl)]

// `Arc` = "atomically reference counted" pointer: lets several threads share
// ownership of the same value. Needed below because the session manager is shared
// between all incoming HTTP requests (handled concurrently on different threads).
use std::sync::Arc;

mod config;
mod my_shelf;

use config::Config;
use my_shelf::MyShelf;

// `use` brings names into scope so we can write `tool` instead of `rmcp::tool`.
// The braces group several imports from the same crate (library).
use rmcp::transport::streamable_http_server::{
    // Config for the HTTP transport (allowed hosts, keep-alive, etc.).
    StreamableHttpServerConfig,
    // The actual HTTP service that speaks MCP. Plugs into axum (the web framework).
    StreamableHttpService,
    // MCP is stateful: a client opens a session, then sends several requests.
    // `LocalSessionManager` keeps those sessions in this process's memory
    // (simple; sessions are lost on restart and can't be shared across servers).
    session::local::LocalSessionManager,
};

// `#[tokio::main]` turns `async fn main` into a normal `main` that starts the tokio
// async runtime. Rust doesn't ship with one; `async` functions need a runtime to
// execute them. We need async because a server spends most of its time waiting
// on the network, and async lets one thread juggle many connections.
#[tokio::main]
// `Result<(), io::Error>` (`std::io::Result<()>`): returns Ok(()) on success, or an
// I/O error (e.g. port already in use). Returning a Result from main lets us use
// `?` below, and Rust prints the error and exits with a non-zero code if it fails.
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;

    // Build the MCP-over-HTTP service. Arguments:
    let mcp = StreamableHttpService::new(
        // 1) A *factory closure* (`|| ...` is an anonymous function with no args).
        //    It is called to create a handler for each new session. `Ok(MyShelf)`
        //    wraps our unit struct in `Ok` because creation is allowed to fail.
        || Ok(MyShelf),
        // 2) The session store, wrapped in `Arc` so it can be shared across threads.
        //    `::default()` builds it with default settings.
        Arc::new(LocalSessionManager::default()),
        // 3) Transport settings; defaults are fine for local development.
        StreamableHttpServerConfig::default(),
    );

    // `axum::Router` maps URL paths to handlers. `nest_service("/mcp", mcp)` says:
    // "every request whose path starts with /mcp is handled by the MCP service".
    // This is why clients must connect to http://127.0.0.1:3000/mcp.
    let app = axum::Router::new().nest_service("/mcp", mcp);

    // Open a TCP socket on port 3000 and start listening.
    // - 127.0.0.1 = localhost only: other machines can't connect (safer for dev;
    //   use 0.0.0.0 to accept outside connections).
    // - `.await` pauses this function until the OS finishes the bind, without
    //   blocking the thread.
    // - `?` = "if this returned an Err, return that error from `main` right now;
    //   otherwise unwrap the Ok value". It's Rust's short form of error handling.
    // The address comes from the configuration (see `Config`).
    let addr = &config.listen_address;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // `println!` (the `!` marks a macro) prints a line to stdout so you know the
    // server is up and which URL to give your MCP client.
    println!("MyShelf MCP server listening on http://{addr}/mcp");

    // Run the HTTP server: accept connections and route them through `app`.
    // This future runs forever (until the process is killed or an error occurs).
    // It is the last expression (no `;`), so its Result is returned from `main`.
    axum::serve(listener, app).await?;
    Ok(())
}
