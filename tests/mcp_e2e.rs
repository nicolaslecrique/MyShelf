//! End-to-end test: starts the real server binary and talks to it as an MCP client
//! over HTTP.
//!
//! WHY end-to-end: unit tests can't catch wiring mistakes (wrong route, broken
//! session handling, tool not registered). Here we exercise exactly what a real
//! client like Claude would do: connect, discover tools, call one.
//!
//! WHY this file lives in `tests/`: Cargo compiles each file in `tests/` as a
//! separate *integration test* crate that can only use the public surface of the
//! project, like an outside user. Since `myshelf` is a binary (no library), we
//! can't import its code; we run the compiled program instead.

// `TcpListener` (blocking, std) is used only to ask the OS for a free port.
use std::net::TcpListener;
// `Child` = a handle on a running subprocess. `Command` = builder to launch one.
// `Stdio` = lets us choose what to do with the subprocess's input/output streams.
use std::process::{Child, Command, Stdio};
// `Duration` = a length of time, used for the polling delay below.
use std::time::Duration;

use rmcp::{
    // `ServiceExt` is a *trait* (a set of methods) that adds `.serve(transport)` to
    // MCP handlers. Calling it on a client handler performs the MCP handshake.
    ServiceExt,
    // The request-parameters type for "call a tool" (tool name + arguments).
    model::CallToolRequestParams,
    // The client side of the HTTP transport: it sends JSON-RPC messages to a URL
    // (this is the counterpart of `StreamableHttpService` in the server).
    transport::StreamableHttpClientTransport,
};

/// A tiny wrapper whose only job is to kill the server when the test ends.
///
/// WHY: if the test fails (panics) halfway, we'd otherwise leave a server process
/// running in the background, holding a port and polluting later runs.
// A tuple struct with one unnamed field: the subprocess handle.
struct ServerGuard(Child);

// `Drop` is a trait whose `drop` method Rust calls automatically when a value goes
// out of scope: at the end of the test, and also while unwinding from a panic.
// This pattern is called RAII (cleanup tied to a value's lifetime).
impl Drop for ServerGuard {
    fn drop(&mut self) {
        // `self.0` is the wrapped `Child`. `kill()` stops the process. `let _ =`
        // deliberately ignores the Result: if it's already dead, we don't care.
        let _ = self.0.kill();
        // `wait()` reaps the dead process so it doesn't linger as a "zombie".
        let _ = self.0.wait();
    }
}

// `#[tokio::test]` is like `#[test]` but starts an async runtime around the test,
// so we can use `.await`. The client library is async, hence we need it.
#[tokio::test]
async fn hello_world_tool_works_end_to_end() {
    // Ask the OS for a free port: binding to port 0 means "pick any unused one".
    // WHY: the server's default port (3000) might be taken (e.g. you're running
    // the server in another terminal), and hardcoding would make the test flaky.
    // `.unwrap()` panics on error, which is fine in tests (a panic = test failure).
    // `.local_addr()` tells us which port the OS actually chose.
    // The listener is a temporary, so it is dropped at the end of this statement,
    // releasing the port for the server to use right after.
    let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();

    // Launch the server and put it straight into the guard so it's always cleaned up.
    let _server = ServerGuard(
        // `env!("CARGO_BIN_EXE_myshelf")` is resolved at compile time by Cargo: it's
        // the path to the freshly built `myshelf` binary. Cargo builds the binary
        // before running integration tests, so we always test the current code.
        Command::new(env!("CARGO_BIN_EXE_myshelf"))
            // Override `listen_address` from the config (env vars take priority over
            // the TOML file), so the server uses our free port instead of 3000.
            .env("MYSHELF_LISTEN_ADDRESS", addr.to_string())
            // Discard the server's startup message so it doesn't clutter test output.
            .stdout(Stdio::null())
            // Actually start the process (non-blocking: it keeps running).
            .spawn()
            // `expect` = `unwrap` with a custom panic message, easier to debug.
            .expect("failed to start server"),
    );
    // The variable starts with `_` to tell the compiler "unused on purpose": we
    // never read it, we only need it to stay alive (and be dropped) until the end.

    // The process starting is not the same as the server being ready to accept
    // connections, so we poll until it is. Otherwise the client could connect
    // too early and fail randomly (a "race condition").
    let mut ready = false;
    // Try up to 100 times; `_` = we don't need the loop counter.
    for _ in 0..100 {
        // Try a plain TCP connection. `is_ok()` is true if the server answered.
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            ready = true;
            // Stop polling as soon as the server is up.
            break;
        }
        // Not ready yet: wait 100 ms (async sleep, doesn't block the thread).
        // Total patience: 100 x 100 ms = 10 s before giving up.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Fail with a clear message if the server never came up (`{addr}` is inlined
    // into the message).
    assert!(ready, "server did not start listening on {addr}");

    // Build the client-side transport pointing at the server's MCP endpoint.
    // The `/mcp` path must match the `nest_service("/mcp", ...)` in `main.rs`.
    let transport = StreamableHttpClientTransport::from_uri(format!("http://{addr}/mcp"));
    // `()` is the simplest possible client handler (it ignores server-initiated
    // requests, which we don't need). `.serve(transport)` connects and performs
    // the MCP handshake (the `initialize` exchange); the result is a client handle
    // we use to send requests.
    let client = ().serve(transport).await.expect("MCP handshake failed");

    // STEP 1 - discovery: ask the server which tools it offers (MCP `tools/list`).
    // `_all_` follows pagination so we get every tool, not just the first page.
    let tools = client.list_all_tools().await.expect("list_tools failed");
    // Check that our tool is advertised. `any` returns true if at least one tool
    // has the right name. The second argument is the failure message; `{tools:?}`
    // prints the whole list in debug format to show what was actually returned.
    assert!(tools.iter().any(|t| t.name == "hello_world"), "tools: {tools:?}");

    // STEP 2 - invocation: call the tool (MCP `tools/call`) by name. `hello_world`
    // takes no arguments, so we only pass the name.
    let result = client
        .call_tool(CallToolRequestParams::new("hello_world"))
        .await
        .expect("call_tool failed");
    // MCP distinguishes "the call failed at protocol level" (the `expect` above)
    // from "the tool ran but reported an error" (`is_error`). We want the latter
    // to be false. `Some(false)` because the field is optional (`Option<bool>`) and
    // the server sets it explicitly.
    assert_eq!(result.is_error, Some(false));
    // A tool result is a list of content items (text, image, ...). We take the
    // first one, require it to be text (`as_text()` returns `None` otherwise),
    // and copy the string out.
    let text = result.content[0].as_text().expect("text content").text.clone();
    // The real check: the server returned exactly what `hello_world` should say.
    assert_eq!(text, "Hello, world! This is MyShelf.");

    // Politely close the MCP session. `.ok()` ignores any error: the test already
    // passed, and the guard will kill the server anyway.
    client.cancel().await.ok();
}
