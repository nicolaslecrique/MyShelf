// BACKGROUND: MCP (Model Context Protocol) is a protocol that lets an AI client
// (e.g. Claude) discover and call "tools" exposed by a server. This file is such
// a server. It serves MCP over HTTP at two endpoints:
// - /mcp, for logged-in users (see `auth.rs`): every tool,
// - /public/mcp, with no login: only the tools that need none (`hello_world`).

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

mod auth;
mod config;
mod logging;
mod my_shelf;

use auth::{Auth, AuthSettings};
use config::{AuthConfig, Config};
use my_shelf::MyShelf;

// `use` brings names into scope so we can write `tool` instead of `rmcp::tool`.
// The braces group several imports from the same crate (library).
use rmcp::transport::streamable_http_server::{
    // Config for the HTTP transport (allowed hosts, keep-alive, etc.).
    StreamableHttpServerConfig,
    // The actual HTTP service that speaks MCP. Plugs into axum (the web framework).
    StreamableHttpService,
    // We run stateless: no sessions are created or stored. Each request is
    // self-contained, so any instance can serve it (restarts, scale-to-zero and
    // multiple instances on Scaleway are all fine). `NeverSessionManager` is the
    // "no sessions" implementation.
    session::never::NeverSessionManager,
};

// `#[tokio::main]` turns `async fn main` into a normal `main` that starts the tokio
// async runtime. Rust doesn't ship with one; `async` functions need a runtime to
// execute them. We need async because a server spends most of its time waiting
// on the network, and async lets one thread juggle many connections.
#[tokio::main]
// `Box<dyn Error>`: "any error type" (e.g. port already in use). Returning a
// Result from main lets us use `?` below, and Rust prints the error to stderr and
// exits with a non-zero code if it fails.
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    logging::init(&config)?;
    // The log field is named after the variable. (Computed first: coverage
    // reports expressions written inside `tracing` macros as missed even when
    // they run.)
    let version = env!("CARGO_PKG_VERSION");
    tracing::info!(version, "starting");

    // `axum::Router` maps URL paths to handlers. `nest_service("/public/mcp", ...)`
    // says: "every request whose path starts with /public/mcp is handled by this
    // MCP service". `/health` is a liveness probe for the platform (e.g.
    // Scaleway's health check).
    let mut app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .nest_service("/public/mcp", mcp_service(MyShelf::public, &config));

    // `/mcp` is served only when login is configured, so that it can never be
    // reached without a token by mistake.
    if let Some(auth_config) = &config.auth {
        let auth =
            Arc::new(Auth::new(auth_settings(auth_config)).map_err(|e| format!("auth: {e}"))?);
        let mcp = axum::Router::new()
            .nest_service("/mcp", mcp_service(MyShelf::logged_in, &config))
            // `.layer` on this small router wraps only `/mcp`: every request
            // goes through `require_login` first.
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&auth),
                auth::require_login,
            ));
        // `merge` adds another router's routes to this one.
        app = app.merge(mcp).merge(auth.routes());
    } else {
        tracing::warn!("auth is not configured: /mcp is disabled, only /public/mcp is served");
    }
    // `.layer(...)` wraps every route with `log_request`.
    let app = app.layer(axum::middleware::from_fn(log_request));

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

    // Logs that the server is up and which URL to give your MCP client.
    // `tracing::info!` (the `!` marks a macro) records an event with named
    // fields; `logging.rs` decides how it is written.
    let path = if config.auth.is_some() {
        "/mcp"
    } else {
        "/public/mcp"
    };
    let url = format!("http://{addr}{path}");
    tracing::info!(url, "listening");

    // Run the HTTP server: accept connections and route them through `app`.
    // This future runs forever (until the process is killed or an error occurs).
    // It is the last expression (no `;`), so its Result is returned from `main`.
    // `with_graceful_shutdown`: when the given future completes, stop accepting new
    // connections and let in-flight requests finish before returning.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("stopped");
    Ok(())
}

/// What `auth.rs` needs, from our configuration. `auth.rs` doesn't read the
/// config itself, so that it doesn't depend on the rest of the app.
fn auth_settings(config: &AuthConfig) -> AuthSettings {
    use std::time::Duration;
    AuthSettings {
        issuer: config.issuer.clone(),
        resource: config.resource.clone(),
        resource_name: "MyShelf".to_string(),
        keys_max_age: Duration::from_secs(config.keys_max_age_secs),
        keys_min_refresh_interval: Duration::from_secs(config.keys_min_refresh_interval_secs),
        http_timeout: Duration::from_secs(config.http_timeout_secs),
    }
}

/// Builds an MCP-over-HTTP service whose handler is created by `new_handler`.
///
/// `fn() -> MyShelf` is the type of a plain function taking no argument and
/// returning a `MyShelf`, e.g. `MyShelf::public`.
fn mcp_service(
    new_handler: fn() -> MyShelf,
    config: &Config,
) -> StreamableHttpService<MyShelf, NeverSessionManager> {
    StreamableHttpService::new(
        // 1) A *factory closure* (`|| ...` is an anonymous function with no args).
        //    It is called to create a handler for each request. It wraps the
        //    handler in `Ok` because creation is allowed to fail. `move` copies
        //    `new_handler` into the closure, which outlives this function.
        move || Ok(new_handler()),
        // 2) The session manager, wrapped in `Arc` so it can be shared across threads.
        //    Here: none (stateless).
        Arc::new(NeverSessionManager::default()),
        // 3) Transport settings:
        //    - `legacy_session_mode(false)`: don't use the old session-based protocol.
        //    - `allowed_hosts`: the default accepts localhost only, which would
        //      reject every request once deployed behind a public hostname.
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_allowed_hosts(config.allowed_hosts.clone()),
    )
}

/// Resolves when the process is asked to stop.
///
/// Why this exists: on scale-down or redeploy, Scaleway sends SIGTERM and, if the
/// process is still alive after a grace period, SIGKILLs it (cutting off any
/// in-flight request). In a container our process is PID 1, and Linux ignores
/// SIGTERM for PID 1 unless a handler is installed, so without this the server
/// would never react and would always be killed the hard way.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    let signal = tokio::select! {
        _ = sigterm.recv() => "SIGTERM",
        // Ctrl-C (SIGINT), so stopping the server locally also exits cleanly.
        _ = tokio::signal::ctrl_c() => "SIGINT",
    };
    tracing::info!(signal, "shutting down, finishing in-flight requests");
}

/// Middleware: logs one line per HTTP request once its response is ready.
///
/// `next.run(request)` passes the request on to the route and gives back its
/// response, so the code around it runs before and after every handler.
async fn log_request(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    // Time until the response headers are ready. For streamed MCP replies the
    // body may still be in flight.
    let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    // `%x` logs `x` with its `Display` (user-facing) format.
    tracing::info!(%method, path, status, duration_ms, "request");
    response
}
