include!(concat!(env!("OUT_DIR"), "/tidal.rs"));

pub const BASE_URL: &str = "https://openapi.tidal.com/v2";

/// Generated client that sends `Authorization: Bearer <token>` on every request.
pub fn client(base: &str, token: &str) -> Client {
    Client::new_with_client(base, crate::auth::bearer_client(token))
}

/// Pre-request hook that sets the JSON:API media type on `Accept` and, for requests with a body, `Content-Type`.
pub async fn vnd_api(r: &mut reqwest::Request) -> Result<(), std::convert::Infallible> {
    let vnd = reqwest::header::HeaderValue::from_static("application/vnd.api+json");
    if r.body().is_some() {
        r.headers_mut()
            .insert(reqwest::header::CONTENT_TYPE, vnd.clone());
    }
    r.headers_mut().insert(reqwest::header::ACCEPT, vnd);
    Ok(())
}

/// Decodes the `tracks` resources of a JSON:API `included` array, skipping any that fail to parse.
pub fn included_tracks(included: &[serde_json::Value]) -> Vec<types::TracksResourceObject> {
    included
        .iter()
        .filter(|v| v["type"] == "tracks")
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}
