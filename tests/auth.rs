//! End-to-end tests of login on `/mcp`: the real server binary, checking tokens
//! signed by a fake identity provider (see `common/provider.rs`).

mod common;

use common::{
    FakeProvider, TestServer,
    provider::{KEY_ID, PUBLISHED_SECRET},
};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

/// The status code of a raw HTTP response, e.g. `401`.
fn status(response: &str) -> u16 {
    response.split(' ').nth(1).unwrap().parse().unwrap()
}

/// The value of header `name` in a raw HTTP response.
fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response
        .lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(": "))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

/// The body of a raw HTTP response.
fn body(response: &str) -> &str {
    response.split_once("\r\n\r\n").unwrap().1
}

/// POSTs to `/mcp` with this `Authorization` header (if any). Every request
/// these tests send this way is refused before reaching MCP, so the body is
/// irrelevant.
async fn post_mcp(server: &TestServer, authorization: Option<&str>) -> String {
    let headers: Vec<_> = authorization
        .map(|value| ("Authorization", value))
        .into_iter()
        .collect();
    server.http_request("POST", "/mcp", &headers, "{}").await
}

/// With a valid token, `/mcp` serves every tool, and tools know the user.
#[tokio::test]
async fn logged_in_user_gets_every_tool() {
    let provider = FakeProvider::start().await;
    let server = TestServer::start_with_auth(&provider).await;
    let token = provider.sign(&provider.claims(&server.resource()));
    let client = server.try_connect("/mcp", Some(&token)).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(names.contains(&"hello_world"), "tools: {names:?}");
    assert!(names.contains(&"hello_me"), "tools: {names:?}");

    let result = client
        .call_tool(CallToolRequestParams::new("hello_me"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].as_text().expect("text content").text;
    let issuer = provider.issuer();
    assert_eq!(
        text,
        &format!("Hello, user-123! You are logged in to MyShelf (identity provider: {issuer}).")
    );

    // Several requests so far (handshake, list, call), one download of the keys.
    assert_eq!(provider.jwks_fetches(), 1);
    client.cancel().await.ok();
}

/// Without a token, `/mcp` answers 401 with where to find how to log in: this
/// is what makes MCP clients (Claude...) start their login flow.
#[tokio::test]
async fn mcp_without_token_points_to_the_login_metadata() {
    let provider = FakeProvider::start().await;
    let server = TestServer::start_with_auth(&provider).await;
    let metadata_url = server
        .resource()
        .replace("/mcp", "/.well-known/oauth-protected-resource/mcp");
    let challenge = format!(r#"Bearer resource_metadata="{metadata_url}""#);

    let response = post_mcp(&server, None).await;
    assert_eq!(status(&response), 401, "{response}");
    assert_eq!(
        header(&response, "www-authenticate"),
        Some(challenge.as_str())
    );

    // Another authentication scheme counts as no token.
    let response = post_mcp(&server, Some("Basic dXNlcjpwYXNz")).await;
    assert_eq!(status(&response), 401, "{response}");
    assert_eq!(
        header(&response, "www-authenticate"),
        Some(challenge.as_str())
    );

    assert!(server.try_connect("/mcp", None).await.is_err());

    // The rest stays reachable without a token.
    let client = server.try_connect("/public/mcp", None).await.unwrap();
    client.cancel().await.ok();
    assert_eq!(status(&server.http_get("/health").await), 200);
    assert_eq!(status(&server.http_get("/nothing-here").await), 404);
}

/// The protected resource metadata (RFC 9728) names the provider to log in with.
#[tokio::test]
async fn protected_resource_metadata_names_the_provider() {
    let provider = FakeProvider::start().await;
    let server = TestServer::start_with_auth(&provider).await;
    for path in [
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-protected-resource",
    ] {
        let response = server.http_get(path).await;
        assert_eq!(status(&response), 200, "{path}: {response}");
        let metadata: Value = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(metadata["resource"], server.resource(), "{path}");
        assert_eq!(
            metadata["authorization_servers"],
            json!([provider.issuer()]),
            "{path}"
        );
    }
}

/// Every kind of bad token gets a 401 `invalid_token`, so the client logs in again.
#[tokio::test]
async fn invalid_tokens_are_rejected() {
    let provider = FakeProvider::start().await;
    let server = TestServer::start_with_auth(&provider).await;
    let valid = provider.claims(&server.resource());
    // A valid token with one claim changed.
    let with = |claim: &str, value: Value| {
        let mut claims = valid.clone();
        claims[claim] = value;
        provider.sign(&claims)
    };
    let mut no_subject = valid.clone();
    no_subject.as_object_mut().unwrap().remove("sub");
    // Claims for user `admin` with the signature of user `user-123`'s token.
    let admin = with("sub", json!("admin"));
    let user = provider.sign(&valid);
    let forged = format!(
        "{}.{}",
        admin.rsplit_once('.').unwrap().0,
        user.rsplit_once('.').unwrap().1
    );
    let mut unknown_key = Header::new(Algorithm::RS256);
    unknown_key.kid = Some("some-other-key".to_string());
    // "Algorithm confusion": HMAC-signed with a secret, claiming the provider's key id.
    let mut hmac = Header::new(Algorithm::HS256);
    hmac.kid = Some(KEY_ID.to_string());
    let hmac_signed = jsonwebtoken::encode(
        &hmac,
        &valid,
        &EncodingKey::from_secret(b"guessable").unwrap(),
    )
    .unwrap();

    // HMAC-signed with a secret the key set itself publishes.
    let mut published = Header::new(Algorithm::HS256);
    published.kid = Some("shared-secret".to_string());
    let published_secret_signed = jsonwebtoken::encode(
        &published,
        &valid,
        &EncodingKey::from_secret(PUBLISHED_SECRET).unwrap(),
    )
    .unwrap();

    let cases = [
        (
            "expired",
            with("exp", json!(jsonwebtoken::get_current_timestamp() - 3600)),
        ),
        (
            "for another server",
            with("aud", json!("https://other.example/mcp")),
        ),
        (
            "from another issuer",
            with("iss", json!("https://evil.example")),
        ),
        ("without subject", provider.sign(&no_subject)),
        ("forged", forged),
        (
            "signed with an unknown key",
            provider.sign_with_header(&unknown_key, &valid),
        ),
        (
            "without key id",
            provider.sign_with_header(&Header::new(Algorithm::RS256), &valid),
        ),
        ("HMAC-signed", hmac_signed),
        ("signed with a published secret", published_secret_signed),
        ("not a JWT", "not-a-jwt".to_string()),
    ];
    for (case, token) in cases {
        let response = post_mcp(&server, Some(&format!("Bearer {token}"))).await;
        assert_eq!(status(&response), 401, "{case}: {response}");
        let challenge = header(&response, "www-authenticate").unwrap();
        assert!(
            challenge.contains(r#"error="invalid_token""#),
            "{case}: {challenge}"
        );
    }

    // One download of the keys for all of these: an unknown key id right after a
    // download does not trigger another (anyone can send such tokens).
    assert_eq!(provider.jwks_fetches(), 1);
}

/// If the provider can't be reached, tokens can't be checked: 503 (retry
/// later), not 401 (which would make clients log the user out).
#[tokio::test]
async fn unreachable_provider_is_a_temporary_error() {
    // An address nothing listens on: bind a free port, then release it.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let issuer = format!("http://{closed}");
    let server = TestServer::start_with(|addr| {
        vec![
            ("MYSHELF_AUTH__ISSUER", issuer.clone()),
            ("MYSHELF_AUTH__RESOURCE", format!("http://{addr}/mcp")),
        ]
    })
    .await;
    // Any well-formed token: it is refused before its signature is checked.
    let provider = FakeProvider::start().await;
    let token = provider.sign(&json!({ "iss": issuer }));

    let response = post_mcp(&server, Some(&format!("Bearer {token}"))).await;
    assert_eq!(status(&response), 503, "{response}");
}

/// The provider's metadata must name the configured issuer exactly, else it
/// can't be trusted for keys: here the config has an extra trailing `/`.
#[tokio::test]
async fn issuer_mismatch_with_provider_metadata_is_refused() {
    let provider = FakeProvider::start().await;
    let issuer = format!("{}/", provider.issuer());
    let server = TestServer::start_with(|addr| {
        vec![
            ("MYSHELF_AUTH__ISSUER", issuer.clone()),
            ("MYSHELF_AUTH__RESOURCE", format!("http://{addr}/mcp")),
        ]
    })
    .await;
    let mut claims = provider.claims(&server.resource());
    claims["iss"] = json!(issuer);

    let response = post_mcp(&server, Some(&format!("Bearer {}", provider.sign(&claims)))).await;
    assert_eq!(status(&response), 503, "{response}");
    assert!(
        server.logs().contains("issuer is"),
        "logs: {}",
        server.logs()
    );
    assert_eq!(provider.jwks_fetches(), 0);
}
