// The MyShelf MCP server: the type that holds our tools and its tool router.

// `tool`, `tool_router` and `tool_handler` are *macros* (code that writes code at
// compile time). They save us from hand-writing the MCP plumbing (tool listing,
// JSON schemas, request dispatch).
use rmcp::{
    ServerHandler, handler::server::router::tool::ToolRouter, tool, tool_handler, tool_router,
};

use crate::auth::AuthUser;

// `#[derive(Clone)]` auto-generates a `.clone()` method. The MCP service creates a
// fresh server handler per request (see the closures in `main`), and the macros
// require the type to be cloneable.
#[derive(Clone)]
// The type that *is* our MCP server; its `#[tool]` methods become MCP tools.
// No state yet (no database etc.), only which tools this instance exposes: the
// same type serves both endpoints, with different tool sets.
pub struct MyShelf {
    tool_router: ToolRouter<Self>,
}

impl MyShelf {
    /// For `/public/mcp`, which needs no login: only the public tools.
    pub fn public() -> Self {
        Self {
            tool_router: Self::public_tools(),
        }
    }

    /// For `/mcp`, behind login: every tool. (`+` merges two tool routers.)
    pub fn logged_in() -> Self {
        Self {
            tool_router: Self::public_tools() + Self::user_tools(),
        }
    }
}

// `#[tool_router(router = public_tools)]` is applied to an `impl` block (where
// methods are defined). It scans the methods marked `#[tool]` and generates a
// function `Self::public_tools()` returning a router that maps tool names to
// those methods. Tools that need no login go here.
#[tool_router(router = public_tools)]
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

// Tools that need a logged-in user. They are only on `/mcp`, where the login
// middleware (`auth::require_login`) has already checked the access token.
#[tool_router(router = user_tools)]
impl MyShelf {
    /// Greet the logged-in user, confirming `MyShelf` knows who they are.
    #[tool]
    // `user: AuthUser`: rmcp fills this parameter from the request (see
    // `FromContextPart` in `auth.rs`), it is not an argument the AI sends. The
    // macro needs it by value, hence the second `allow`.
    #[allow(clippy::unused_self, clippy::needless_pass_by_value)]
    fn hello_me(&self, user: AuthUser) -> String {
        format!(
            "Hello, {}! You are logged in to MyShelf (identity provider: {}).",
            user.subject, user.issuer
        )
    }
}

// The implementation of rmcp's `ServerHandler` trait, i.e. the glue that answers
// MCP requests like "list tools"/"call tool", using this instance's tool router.
// Without this, `MyShelf` would be a plain struct that MCP clients can't talk to.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for MyShelf {}
