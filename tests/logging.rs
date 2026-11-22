//! End-to-end test of the logs the server writes to stdout.

mod common;

use common::TestServer;
use serde_json::Value;

/// Parses JSON log lines and returns the first one whose `message` is `message`.
fn find_event(logs: &str, message: &str) -> Value {
    logs.lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("log line is not JSON"))
        .find(|event| event["message"] == message)
        .unwrap_or_else(|| panic!("no {message:?} event in logs:\n{logs}"))
}

/// The lifecycle events and one event per request, as JSON lines.
#[tokio::test]
async fn json_logs_cover_startup_requests_and_shutdown() {
    let mut server = TestServer::start().await;
    server.http_get("/health").await;
    assert!(server.stop_with("TERM"), "server did not exit cleanly");
    let logs = server.logs();

    let starting = find_event(&logs, "starting");
    assert_eq!(starting["level"], "INFO");
    assert!(starting["version"].is_string(), "{starting}");

    find_event(&logs, "listening");

    let request = find_event(&logs, "request");
    assert_eq!(request["method"], "GET");
    assert_eq!(request["path"], "/health");
    assert_eq!(request["status"], 200);
    assert!(request["duration_ms"].is_f64(), "{request}");

    assert_eq!(
        find_event(&logs, "shutting down, finishing in-flight requests")["signal"],
        "SIGTERM"
    );
    find_event(&logs, "stopped");
}
