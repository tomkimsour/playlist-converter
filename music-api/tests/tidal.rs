#[allow(dead_code)]
mod common;

use common::{Prism, ok, tidal_prism_spec};
use music_api::tidal::{self, types};
use serde_json::{from_value, json};

const PLAYLIST: &str = "123e4567-e89b-12d3-a456-426614174000";

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn should_get_2xx_and_decode_when_every_call_hits_tidal_prism() {
    let prism = Prism::start(&tidal_prism_spec());
    let c = tidal::client(&prism.url, "test-token");
    let country = Some("US");

    ok("get_users_id", c.get_users_id("me", None, None, None).await).await;
    let owners = vec![
        "me".parse::<types::GetPlaylistsFilterOwnersIdItem>()
            .unwrap(),
    ];
    ok(
        "get_playlists",
        c.get_playlists(
            country,
            None,
            None,
            Some(&owners),
            None,
            None,
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    ok(
        "get_playlists_id",
        c.get_playlists_id(PLAYLIST, country, None, None, None)
            .await,
    )
    .await;
    let items = strings(&["items"]);
    ok(
        "get_playlists_id_relationships_items",
        c.get_playlists_id_relationships_items(
            PLAYLIST,
            country,
            None,
            Some(&items),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let isrc = vec![
        "USUM71703861"
            .parse::<types::GetTracksFilterIsrcItem>()
            .unwrap(),
    ];
    ok(
        "get_tracks",
        c.get_tracks(
            country,
            None,
            Some(&isrc),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let query: types::GetSearchResultsFilterQuery = "daft punk one more time".parse().unwrap();
    let tracks = strings(&["tracks"]);
    ok(
        "get_search_results",
        c.get_search_results(
            None,
            country,
            None,
            None,
            &query,
            Some(&tracks),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let body = from_value(json!({"data": {"type": "playlists", "attributes": {
        "name": "contract test", "description": "contract test", "accessType": "UNLISTED"}}}))
    .unwrap();
    ok("post_playlists", c.post_playlists(None, &body).await).await;
    let body = from_value(json!({"data": [
        {"id": "75413016", "type": "tracks"}, {"id": "75413017", "type": "tracks"}]}))
    .unwrap();
    ok(
        "post_playlists_id_relationships_items",
        c.post_playlists_id_relationships_items(PLAYLIST, None, &body)
            .await,
    )
    .await;
    let body = from_value(
        json!({"data": {"id": PLAYLIST, "type": "playlists", "attributes": {"name": "renamed"}}}),
    )
    .unwrap();
    if let Err(e) = c.patch_playlists_id(PLAYLIST, None, &body).await {
        common::fail("patch_playlists_id", e).await;
    }
    ok(
        "get_albums_id",
        c.get_albums_id("251380836", country, None, None, None, None)
            .await,
    )
    .await;
    ok(
        "get_artists_id",
        c.get_artists_id("1566", country, None, None, None).await,
    )
    .await;
}

#[test]
fn should_return_only_the_track_when_included_mixes_resource_types() {
    let raw = include_str!("fixtures/tidal_playlist_items.json");
    let doc: types::PlaylistsItemsMultiRelationshipDataDocument =
        serde_json::from_str(raw).unwrap();
    assert_eq!(doc.data.len(), 2);
    let types::Included(included) = doc.included.unwrap();
    let tracks = tidal::included_tracks(&included);
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        tracks[0]
            .attributes
            .as_ref()
            .unwrap()
            .isrc
            .as_deref()
            .map(String::as_str),
        Some("FRZ110002730")
    );
}
