//! Shared setup for end-to-end tests: launches the real server binary and connects
//! an MCP client to it.
//!
//! WHY `tests/common/mod.rs` and not `tests/common.rs`: Cargo compiles every
//! top-level file in `tests/` as its own test crate. A file inside a subdirectory is
//! not compiled on its own; test files pull it in with `mod common;`.
//!
//! Each test file compiles its own copy of this module and uses only part of it,
//! so unused-code warnings here are expected.
#![allow(dead_code)]

// `TcpListener` (blocking, std) is used only to ask the OS for a free port.
use std::fs::File;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
// `Child` = a handle on a running subprocess. `Command` = builder to launch one.
// `Stdio` = lets us choose what to do with the subprocess's input/output streams.
use std::process::{Child, Command, Stdio};
// `Duration` = a length of time, used for the polling delay below.
use std::time::Duration;

use rmcp::{
    // `ServiceExt` is a *trait* (a set of methods) that adds `.serve(transport)` to
    // MCP handlers. Calling it on a client handler performs the MCP handshake.
    ServiceExt,
    service::{RoleClient, RunningService},
    // The client side of the HTTP transport: it sends JSON-RPC messages to a URL
    // (this is the counterpart of `StreamableHttpService` in the server).
    transport::StreamableHttpClientTransport,
};

/// The client handle returned by [`TestServer::connect`].
pub type Client = RunningService<RoleClient, ()>;

/// A running server process, killed automatically when this value is dropped.
///
/// WHY: if a test fails (panics) halfway, we'd otherwise leave a server process
/// running in the background, holding a port and polluting later runs.
///
/// `Drop` is a trait whose `drop` method Rust calls automatically when a value goes
/// out of scope: at the end of the test, and also while unwinding from a panic.
/// This pattern is called RAII (cleanup tied to a value's lifetime).
pub struct TestServer {
    child: Child,
    addr: SocketAddr,
    /// File receiving the server's stdout (its logs), see [`TestServer::logs`].
    log_file: PathBuf,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Ask the server to stop with SIGTERM (like the platform does) rather than
        // SIGKILL: a gracefully exiting process flushes its code-coverage data,
        // which a killed one cannot. We shell out to `kill` because sending a
        // signal from Rust needs `unsafe` (denied in this project) or a dependency.
        let _ = Command::new("kill")
            .arg(self.child.id().to_string())
            .status();
        // `wait()` reaps the process so it doesn't linger as a "zombie". If the
        // server ignored SIGTERM this would hang, so fall back to SIGKILL first
        // when it has not exited after ~5 s.
        for _ in 0..50 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.log_file);
    }
}

impl TestServer {
    /// Starts the server on a free port and waits until it accepts connections.
    pub async fn start() -> Self {
        // Ask the OS for a free port: binding to port 0 means "pick any unused one".
        // WHY: the server's default port (3000) might be taken (e.g. you're running
        // the server in another terminal), and hardcoding would make tests flaky.
        // The listener is a temporary, so it is dropped at the end of this statement,
        // releasing the port for the server to use right after.
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();

        // The server logs to stdout. We send that to a file rather than a pipe:
        // a pipe nobody reads fills up and then blocks the server on its next log.
        // The port makes the name unique among concurrently running tests.
        let log_file = std::env::temp_dir().join(format!("myshelf-test-{}.log", addr.port()));

        // `env!("CARGO_BIN_EXE_myshelf")` is resolved at compile time by Cargo: it's
        // the path to the freshly built `myshelf` binary. Cargo builds the binary
        // before running integration tests, so we always test the current code.
        let child = Command::new(env!("CARGO_BIN_EXE_myshelf"))
            // Override `listen_address` from the config (env vars take priority over
            // the TOML file), so the server uses our free port instead of 3000.
            .env("MYSHELF_LISTEN_ADDRESS", addr.to_string())
            // WHY: point the config file at a path that doesn't exist, so tests run
            // on the built-in defaults and a developer's local `myshelf.toml`
            // (e.g. `log_filter = "trace"`) can't change their behaviour.
            .env("MYSHELF_CONFIG", "/nonexistent/myshelf.toml")
            .stdout(Stdio::from(File::create(&log_file).unwrap()))
            // Actually start the process (non-blocking: it keeps running).
            .spawn()
            .expect("failed to start server");

        // Build the guard right away so the process is cleaned up even if the
        // readiness check below panics.
        let server = Self {
            child,
            addr,
            log_file,
        };

        // The process starting is not the same as the server being ready to accept
        // connections, so we poll until it is. Otherwise the client could connect
        // too early and fail randomly (a "race condition").
        let mut ready = false;
        // Try up to 100 times; `_` = we don't need the loop counter.
        for _ in 0..100 {
            // Try a plain TCP connection. `is_ok()` is true if the server answered.
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                ready = true;
                break;
            }
            // Not ready yet: wait 100 ms. Total patience: 10 s before giving up.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(ready, "server did not start listening on {addr}");

        server
    }

    /// Sends `signal` (e.g. `"INT"`) to the server and waits up to 5 s for it to
    /// exit; returns whether it exited successfully.
    pub fn stop_with(&mut self, signal: &str) -> bool {
        let _ = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(self.child.id().to_string())
            .status();
        for _ in 0..50 {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status.success();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// Everything the server has written to stdout so far.
    pub fn logs(&self) -> String {
        std::fs::read_to_string(&self.log_file).unwrap()
    }

    /// Sends a minimal HTTP GET and returns the raw response text.
    pub async fn http_get(&self, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    /// Connects an MCP client and performs the handshake.
    pub async fn connect(&self) -> Client {
        // The `/mcp` path must match the `nest_service("/mcp", ...)` in `main.rs`.
        let transport =
            StreamableHttpClientTransport::from_uri(format!("http://{}/mcp", self.addr));
        // `()` is the simplest possible client handler (it ignores server-initiated
        // requests, which we don't need). `.serve(transport)` performs the MCP
        // handshake (the `initialize` exchange).
        ().serve(transport).await.expect("MCP handshake failed")
    }
}
