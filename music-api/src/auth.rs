use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

fn random_string(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| UNRESERVED[rng.random_range(0..UNRESERVED.len())] as char)
        .collect()
}

/// Random `state` value for the OAuth redirect check.
pub fn random_state() -> String {
    random_string(32)
}

/// PKCE verifier and its S256 challenge (RFC 7636).
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        Self::from_verifier(&random_string(64))
    }

    pub fn from_verifier(verifier: &str) -> Self {
        Self {
            verifier: verifier.to_owned(),
            challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier)),
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses an HTTP request line. `Ok(None)` means the path is not `/callback`;
/// `Ok(Some(code))` is a valid redirect; errors cover provider errors, state
/// mismatch and a missing code.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Result<Option<String>, Error> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))?;
    if url.path() != "/callback" {
        return Ok(None);
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    if let Some(e) = param("error").or_else(|| param("error_reason")) {
        return Err(format!("authorization failed: {e}").into());
    }
    if param("state").as_deref() != Some(expected_state) {
        return Err("state mismatch".into());
    }
    param("code").map(Some).ok_or_else(|| "missing code".into())
}

/// Listens on 127.0.0.1:8888 until the browser hits `/callback`, then returns the authorization code.
pub async fn wait_for_code(state: &str) -> Result<String, Error> {
    let listener = TcpListener::bind("127.0.0.1:8888").await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let mut n = 0;
        while n < buf.len() && !buf[..n].windows(2).any(|w| w == b"\r\n") {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf[n..])).await {
                Ok(Ok(read)) if read > 0 => n += read,
                _ => break,
            }
        }
        let head = String::from_utf8_lossy(&buf[..n]);
        let result = parse_callback(head.lines().next().unwrap_or(""), state);
        let (status, body) = match result {
            Ok(None) => ("404 Not Found", "Not found."),
            Ok(Some(_)) => ("200 OK", "You can close this tab."),
            Err(_) => ("400 Bad Request", "Login failed, see the terminal."),
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(code) = result? {
            return Ok(code);
        }
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);

/// HTTP client that gives up on a request after 30 seconds.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(TIMEOUT).build().unwrap()
}

/// HTTP client that sends `Authorization: Bearer <token>` on every request and gives up after 30 seconds.
pub fn bearer_client(token: &str) -> reqwest::Client {
    let mut auth = reqwest::header::HeaderValue::try_from(format!("Bearer {token}")).unwrap();
    auth.set_sensitive(true);
    let headers = reqwest::header::HeaderMap::from_iter([(reqwest::header::AUTHORIZATION, auth)]);
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .default_headers(headers)
        .build()
        .unwrap()
}

#[derive(Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// POSTs a form-encoded token request and parses the JSON response.
pub async fn exchange(
    http: &reqwest::Client,
    token_url: &str,
    form: &[(&str, &str)],
) -> Result<Token, Error> {
    let resp = http.post(token_url).form(form).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("token request failed: {status}: {body}").into());
    }
    Ok(resp.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = "xyz";

    fn line(query: &str) -> String {
        format!("GET /callback?{query} HTTP/1.1")
    }

    #[test]
    fn should_match_rfc7636_vector_when_verifier_given() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn should_generate_unreserved_64_chars_when_new() {
        let (a, b) = (Pkce::new(), Pkce::new());
        assert_eq!(a.verifier.len(), 64);
        assert!(
            a.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c))
        );
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(random_state(), random_state());
    }

    #[test]
    fn should_return_code_when_callback_valid() {
        let r = parse_callback(&line("code=abc&state=xyz"), STATE).unwrap();
        assert_eq!(r.as_deref(), Some("abc"));
    }

    #[test]
    fn should_decode_code_when_percent_encoded() {
        let r = parse_callback(&line("code=a%2Fb%20c&state=xyz"), STATE).unwrap();
        assert_eq!(r.as_deref(), Some("a/b c"));
    }

    #[test]
    fn should_error_when_state_mismatch() {
        assert!(parse_callback(&line("code=abc&state=bad"), STATE).is_err());
    }

    #[test]
    fn should_error_when_provider_reports_error() {
        let e = parse_callback(&line("error=access_denied&state=xyz"), STATE).unwrap_err();
        assert!(e.to_string().contains("access_denied"));
        let e = parse_callback(&line("error_reason=user_denied"), STATE).unwrap_err();
        assert!(e.to_string().contains("user_denied"));
    }

    #[test]
    fn should_error_when_code_missing() {
        assert!(parse_callback(&line("state=xyz"), STATE).is_err());
    }

    #[test]
    fn should_return_none_when_path_not_callback() {
        let r = parse_callback("GET /favicon.ico HTTP/1.1", STATE).unwrap();
        assert_eq!(r, None);
    }

    #[tokio::test]
    async fn should_return_code_and_answer_200_when_browser_redirects() {
        let server = tokio::spawn(wait_for_code(STATE));
        let mut stream = loop {
            match tokio::net::TcpStream::connect("127.0.0.1:8888").await {
                Ok(s) => break s,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            }
        };
        stream
            .write_all(b"GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).await.unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("You can close this tab."));
        assert_eq!(server.await.unwrap().unwrap(), "abc");
    }
}
