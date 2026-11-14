// The MyShelf MCP server: the type that holds our tools and its tool router.

// `tool` and `tool_router` are *macros* (code that writes code at compile time).
// They save us from hand-writing the MCP plumbing (tool listing, JSON schemas,
// request dispatch).
use rmcp::{tool, tool_router};

// `#[derive(Clone)]` auto-generates a `.clone()` method. The MCP service creates a
// fresh server handler per session (see the closure in `main`), and the macros
// require the type to be cloneable.
#[derive(Clone)]
// A "unit struct": a struct with no fields. We have no state yet (no database etc.).
// This is the type that *is* our MCP server; its methods become MCP tools.
pub struct MyShelf;

// `#[tool_router(server_handler)]` is applied to an `impl` block (where methods
// are defined). It scans the methods marked `#[tool]` and generates:
//  - a router mapping tool names to methods, and
//  - (because of `server_handler`) the implementation of rmcp's `ServerHandler`
//    trait, i.e. the glue that answers MCP requests like "list tools"/"call tool".
// Without this, `MyShelf` would be a plain struct that MCP clients can't talk to.
#[tool_router(server_handler)]
impl MyShelf {
    // `///` is a *doc comment*. rmcp reuses it as the tool's description, which
    // is sent to the AI client so it knows WHEN to call this tool. So write
    // these for the model, not just for humans.
    /// Say hello from `MyShelf`.
    // Marks this method as an MCP tool. The tool name defaults to the function
    // name (`hello_world`).
    #[tool]
    // Clippy pedantic warns when a method takes `&self` but never uses it. The
    // macro needs the `&self` signature anyway, so we silence that warning.
    #[allow(clippy::unused_self)]
    // `fn` declares a function. `&self` = borrow the instance read-only (no
    // ownership taken). `-> String` is the return type; rmcp converts it into an
    // MCP text response. No `;` or `return` on the last line: in Rust the final
    // expression of a block is its return value.
    fn hello_world(&self) -> String {
        // A string literal is a `&str` (borrowed text baked into the binary).
        // `.to_string()` makes an owned `String`, which is what we promised to return.
        "Hello, world! This is MyShelf.".to_string()
    }
}
