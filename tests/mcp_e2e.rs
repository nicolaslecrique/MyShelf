//! End-to-end test of the `hello_world` MCP tool: talks to the real server binary
//! as an MCP client over HTTP.
//!
//! WHY end-to-end: unit tests can't catch wiring mistakes (wrong route, broken
//! session handling, tool not registered). Here we exercise exactly what a real
//! client like Claude would do: connect, discover tools, call one.
//!
//! The server/client setup lives in `common/mod.rs`. Since `myshelf` is a binary
//! (no library), we can't import its code; we run the compiled program instead.

mod common;

use common::TestServer;
// The request-parameters type for "call a tool" (tool name + arguments).
use rmcp::model::CallToolRequestParams;

// `#[tokio::test]` is like `#[test]` but starts an async runtime around the test,
// so we can use `.await`. The client library is async, hence we need it.
#[tokio::test]
async fn hello_world_tool_works_end_to_end() {
    // Kept alive until the end of the test; dropping it kills the server.
    let server = TestServer::start().await;
    let client = server.connect().await;

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
    // passed, and the server is killed when `server` is dropped.
    client.cancel().await.ok();
}
