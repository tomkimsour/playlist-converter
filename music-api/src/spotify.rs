use base64::Engine;

include!(concat!(env!("OUT_DIR"), "/spotify.rs"));

pub const BASE_URL: &str = "https://api.spotify.com/v1";

/// Generated client that sends `Authorization: Bearer <token>` on every request.
pub fn client(base: &str, token: &str) -> Client {
    Client::new_with_client(base, crate::auth::bearer_client(token))
}

/// Replaces a playlist's cover image (`PUT /playlists/{playlist_id}/images`) with a JPEG.
pub async fn upload_playlist_cover(
    client: &Client,
    playlist_id: &str,
    jpeg: &[u8],
) -> Result<(), Error> {
    let response = client
        .client()
        .put(format!(
            "{}/playlists/{}/images",
            client.baseurl(),
            encode_path(playlist_id)
        ))
        .header(reqwest::header::CONTENT_TYPE, "image/jpeg")
        .body(base64::engine::general_purpose::STANDARD.encode(jpeg))
        .send()
        .await?;
    match response.status().as_u16() {
        202 => Ok(()),
        _ => Err(Error::UnexpectedResponse(response)),
    }
}
