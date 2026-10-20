#[allow(dead_code)]
mod common;

use common::{Prism, fail, ok};
use music_api::spotify::{self, types};
use std::path::Path;

const PLAYLIST: &str = "3cEYpjA9oz9GiPac4AsH4n";
const TRACK: &str = "11dFghVXANMlKmJXsNCbNl";
const URIS: [&str; 2] = [
    "spotify:track:4iV5W9uYEdYUVa79Axb7Rh",
    "spotify:track:1301WleyT98MSxVHPZCA6M",
];
const JPEG: &[u8] = &[
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0xff, 0xd9,
];

#[tokio::test]
async fn should_get_2xx_and_decode_when_every_call_hits_spotify_prism() {
    let prism = Prism::start(&Path::new(env!("CARGO_MANIFEST_DIR")).join("specs/spotify.json"));
    let c = spotify::client(&prism.url, "test-token");
    let track = vec![types::ItemTypeItem::Track];

    ok(
        "get_current_users_profile",
        c.get_current_users_profile().await,
    )
    .await;
    ok(
        "get_playlist",
        c.get_playlist(PLAYLIST, None, None, None).await,
    )
    .await;
    ok(
        "get_playlists_items",
        c.get_playlists_items(PLAYLIST, None, None, None, None, None)
            .await,
    )
    .await;
    ok(
        "search isrc",
        c.search(None, None, None, None, "isrc:USUM71703861", &track)
            .await,
    )
    .await;
    ok(
        "search text",
        c.search(
            None,
            None,
            None,
            None,
            "track:One More Time artist:Daft Punk",
            &track,
        )
        .await,
    )
    .await;
    ok("get_track", c.get_track(TRACK, None).await).await;
    let body = types::CreatePlaylistBody {
        collaborative: None,
        description: Some("contract test".into()),
        name: "contract test".into(),
        public: Some(false),
    };
    ok("create_playlist", c.create_playlist(&body).await).await;
    let body = types::AddItemsToPlaylistBody {
        position: None,
        uris: URIS.iter().map(|s| s.to_string()).collect(),
    };
    ok(
        "add_items_to_playlist",
        c.add_items_to_playlist(PLAYLIST, None, None, &body).await,
    )
    .await;
    if let Err(e) = spotify::upload_playlist_cover(&c, PLAYLIST, JPEG).await {
        fail("upload_playlist_cover", e).await;
    }
    ok(
        "get_an_album",
        c.get_an_album("4aawyAB9vmqN3uQ7FjRGTy", None).await,
    )
    .await;
    ok(
        "get_an_artist",
        c.get_an_artist("0TnOYISbd1XYRBk9myaseg").await,
    )
    .await;
}
