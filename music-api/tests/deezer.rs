use music_api::deezer::{Deezer, Error, Paging, PlaylistUpdate, TrackUpdate, authorize_url};

const PLAYLIST: i64 = 11241913244;
const ISRC: &str = "USUM71703861";
const ALBUM: i64 = 302127;
const ARTIST: i64 = 27;
const TRACK: i64 = 3135556;
const USER: &str = "2529";

fn dz() -> Deezer {
    Deezer::new()
}

fn p() -> Paging {
    Paging::default()
}

#[tokio::test]
async fn should_decode_track_when_fetched_by_id_or_isrc() {
    let dz = dz();
    let by_id = dz.track(TRACK).await.unwrap();
    assert_eq!(by_id.id, TRACK);
    assert!(by_id.isrc.is_some());
    assert!(by_id.artist.unwrap().name.is_some());
    assert!(by_id.album.unwrap().title.is_some());

    let by_isrc = dz.track_isrc(ISRC).await.unwrap();
    assert_eq!(by_isrc.isrc.as_deref(), Some(ISRC));
}

#[tokio::test]
async fn should_decode_album_when_fetched_by_id_or_upc() {
    let dz = dz();
    let album = dz.album(ALBUM).await.unwrap();
    assert_eq!(album.id, ALBUM);
    assert_eq!(
        album.tracks.as_ref().unwrap().data.len() as u64,
        album.nb_tracks.unwrap()
    );
    let upc = album.upc.unwrap();

    assert_eq!(dz.album_upc(&upc).await.unwrap().id, ALBUM);
    assert!(!dz.album_tracks(ALBUM, p()).await.unwrap().data.is_empty());
    dz.album_fans(ALBUM, p()).await.unwrap();
}

#[tokio::test]
async fn should_decode_artist_connections() {
    let dz = dz();
    assert_eq!(dz.artist(ARTIST).await.unwrap().id, ARTIST);
    assert_eq!(dz.artist_top(ARTIST, p()).await.unwrap().data.len(), 5);
    assert!(!dz.artist_albums(ARTIST, p()).await.unwrap().data.is_empty());
    assert!(
        !dz.artist_related(ARTIST, p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(!dz.artist_radio(ARTIST, p()).await.unwrap().data.is_empty());
    assert!(
        !dz.artist_playlists(ARTIST, p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    dz.artist_fans(ARTIST, p()).await.unwrap();
}

#[tokio::test]
async fn should_page_every_track_when_playlist_fetched() {
    let dz = dz();
    let playlist = dz.playlist(PLAYLIST).await.unwrap();
    assert_eq!(playlist.id, PLAYLIST);
    let nb = playlist.nb_tracks.unwrap() as usize;

    let all = dz.all(|p| dz.playlist_tracks(PLAYLIST, p)).await.unwrap();
    assert_eq!(all.len(), nb);
    assert!(all.iter().all(|t| t.isrc.is_some()));

    let small = dz
        .all(|pg| {
            dz.playlist_tracks(
                PLAYLIST,
                Paging {
                    index: pg.index,
                    limit: Some(7),
                },
            )
        })
        .await
        .unwrap();
    assert_eq!(small.len(), nb);

    dz.playlist_fans(PLAYLIST, p()).await.unwrap();
    dz.playlist_radio(PLAYLIST, p()).await.unwrap();
}

#[tokio::test]
async fn should_decode_chart_and_chart_connections() {
    let dz = dz();
    let chart = dz.chart(0).await.unwrap();
    assert!(!chart.tracks.unwrap().data.is_empty());
    assert!(!chart.podcasts.unwrap().data.is_empty());
    assert!(!dz.chart_tracks(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.chart_albums(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.chart_artists(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.chart_playlists(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.chart_podcasts(0, p()).await.unwrap().data.is_empty());
}

#[tokio::test]
async fn should_decode_editorial_and_genre() {
    let dz = dz();
    assert!(!dz.editorials(p()).await.unwrap().data.is_empty());
    assert_eq!(dz.editorial(0).await.unwrap().id, 0);
    assert!(
        !dz.editorial_selection(0, None, p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.editorial_selection(0, Some("2020-01-01"), p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(dz.editorial_charts(0).await.unwrap().tracks.is_some());
    dz.editorial_releases(0, p()).await.unwrap();

    assert!(!dz.genres(p()).await.unwrap().data.is_empty());
    assert_eq!(dz.genre(0).await.unwrap().id, 0);
    assert!(!dz.genre_artists(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.genre_podcasts(0, p()).await.unwrap().data.is_empty());
    assert!(!dz.genre_radios(0, p()).await.unwrap().data.is_empty());
}

#[tokio::test]
async fn should_decode_podcast_and_episode() {
    let dz = dz();
    let podcasts = dz.chart_podcasts(0, p()).await.unwrap().data;
    let mut episode = None;
    for pod in podcasts {
        assert_eq!(dz.podcast(pod.id).await.unwrap().id, pod.id);
        if let Some(e) = dz.podcast_episodes(pod.id, p()).await.unwrap().data.first() {
            episode = Some(e.id);
            break;
        }
    }
    let id = episode.expect("a chart podcast with episodes");
    let ep = dz.episode(id).await.unwrap();
    assert_eq!(ep.id, id);
    assert!(ep.podcast.is_some());
}

#[tokio::test]
async fn should_decode_radio() {
    let dz = dz();
    assert!(!dz.radios(p()).await.unwrap().data.is_empty());
    assert_eq!(dz.radio(6).await.unwrap().id, 6);
    assert!(
        !dz.radio_genres(p()).await.unwrap().data[0]
            .radios
            .is_empty()
    );
    assert!(!dz.radio_tracks(6, p()).await.unwrap().data.is_empty());
    assert!(!dz.radio_lists(p()).await.unwrap().data.is_empty());
}

#[tokio::test]
async fn should_decode_search_results() {
    let dz = dz();
    let tracks = dz.search("eminem", p()).await.unwrap();
    assert!(tracks.total.unwrap() > 0 && !tracks.data.is_empty());
    assert!(
        !dz.search_track("eminem", p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.search_album("eminem", p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.search_artist("eminem", p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.search_playlist("eminem", p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.search_podcast("eminem", p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(!dz.search_user("eminem", p()).await.unwrap().data.is_empty());
    dz.search_radio("eminem", p()).await.unwrap();
}

#[tokio::test]
async fn should_apply_strict_and_order_when_searching() {
    let dz = dz();
    let q = music_api::deezer::SearchQuery {
        q: "track:\"i need a dollar\"",
        strict: true,
        order: Some("RANKING"),
    };
    let hits = dz.search_track(q, p()).await.unwrap().data;
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|t| {
        t.title
            .as_deref()
            .unwrap()
            .to_lowercase()
            .contains("i need a dollar")
    }));
}

#[tokio::test]
async fn should_decode_public_user_data() {
    let dz = dz();
    assert_eq!(dz.user(USER).await.unwrap().id, 2529);
    assert!(!dz.user_albums(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_artists(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_playlists(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_tracks(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_followers(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_followings(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_flow(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_podcasts(USER, p()).await.unwrap().data.is_empty());
    assert!(!dz.user_charts(USER, p()).await.unwrap().data.is_empty());
    assert!(
        !dz.user_charts_tracks(USER, p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    assert!(
        !dz.user_charts_artists(USER, p())
            .await
            .unwrap()
            .data
            .is_empty()
    );
    dz.user_charts_albums(USER, p()).await.unwrap();
    dz.user_charts_playlists(USER, p()).await.unwrap();
    dz.user_radios(USER, p()).await.unwrap();
}

#[tokio::test]
async fn should_decode_infos_and_oembed() {
    let dz = dz();
    let infos = dz.infos().await.unwrap();
    assert!(infos.country_iso.is_some());
    assert!(infos.upload_token.is_some());

    let o = dz
        .oembed("https://www.deezer.com/album/302127")
        .await
        .unwrap();
    assert_eq!(o.entity.as_deref(), Some("album"));
    assert_eq!(o.id, Some(ALBUM));
    assert!(o.html.is_some());
}

#[tokio::test]
async fn should_map_errors_to_api_error() {
    let missing = dz().track(0).await.unwrap_err();
    assert!(matches!(missing, Error::Api { code: 800, .. }));

    let unknown = Deezer::with_base("https://api.deezer.com/nonexistent");
    let err = unknown.genre(0).await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 600, .. }), "{err}");

    let err = dz().user("me").await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 200, .. }), "{err}");
}

fn assert_rejected<T: std::fmt::Debug>(name: &str, r: music_api::deezer::Result<T>) {
    match r {
        Err(Error::Api { code: 200, .. }) => {}
        Err(Error::Api { code: 0, kind, .. }) if kind == "OAuthException" => {}
        other => panic!("{name}: expected OAuthException, got {other:?}"),
    }
}

#[tokio::test]
async fn should_reject_every_write_when_no_token() {
    let dz = dz();
    let ids = [1, 2];
    assert_rejected("episode_bookmark_set", dz.episode_bookmark_set(1, 0).await);
    assert_rejected(
        "episode_bookmark_remove",
        dz.episode_bookmark_remove(1).await,
    );
    let update = PlaylistUpdate {
        title: Some("t"),
        description: Some("d"),
        public: Some(true),
        collaborative: Some(false),
    };
    assert_rejected(
        "playlist_update",
        dz.playlist_update(PLAYLIST, &update).await,
    );
    assert_rejected("playlist_delete", dz.playlist_delete(PLAYLIST).await);
    assert_rejected("playlist_seen", dz.playlist_seen(PLAYLIST).await);
    assert_rejected(
        "playlist_add_tracks",
        dz.playlist_add_tracks(PLAYLIST, &ids).await,
    );
    assert_rejected(
        "playlist_reorder_tracks",
        dz.playlist_reorder_tracks(PLAYLIST, &ids).await,
    );
    assert_rejected(
        "playlist_remove_tracks",
        dz.playlist_remove_tracks(PLAYLIST, &ids).await,
    );
    let track = TrackUpdate {
        title: Some("t"),
        artist: Some("a"),
        album: Some("b"),
    };
    let err = dz.track_update(TRACK, &track).await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 500, .. }), "{err}");
    let err = dz.track_delete(TRACK).await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 800, .. }), "{err}");
    assert_rejected("user_add_albums", dz.user_add_albums("me", &ids).await);
    assert_rejected("user_remove_album", dz.user_remove_album("me", 1).await);
    assert_rejected("user_add_artists", dz.user_add_artists("me", &ids).await);
    assert_rejected("user_remove_artist", dz.user_remove_artist("me", 1).await);
    assert_rejected(
        "user_add_playlists",
        dz.user_add_playlists("me", &ids).await,
    );
    assert_rejected(
        "user_remove_playlist",
        dz.user_remove_playlist("me", 1).await,
    );
    assert_rejected("user_add_podcast", dz.user_add_podcast("me", 1).await);
    assert_rejected("user_remove_podcast", dz.user_remove_podcast("me", 1).await);
    assert_rejected("user_add_radio", dz.user_add_radio("me", 1).await);
    assert_rejected("user_remove_radio", dz.user_remove_radio("me", 1).await);
    assert_rejected("user_add_tracks", dz.user_add_tracks("me", &ids).await);
    assert_rejected("user_remove_track", dz.user_remove_track("me", 1).await);
    assert_rejected("user_follow", dz.user_follow("me", 1).await);
    assert_rejected("user_unfollow", dz.user_unfollow("me", 1).await);
    let err = dz.user_notify("me", "hi").await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 800, .. }), "{err}");
    assert_rejected("create_playlist", dz.create_playlist("me", "t").await);
    let err = dz
        .upload_cover(PLAYLIST, vec![0xff, 0xd8])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { code: 300, .. }), "{err}");
}

#[tokio::test]
async fn should_reject_every_token_only_read_when_no_token() {
    let dz = dz();
    let me = "me";
    assert_rejected("user", dz.user(me).await);
    assert_rejected("user_albums", dz.user_albums(me, p()).await);
    assert_rejected("user_artists", dz.user_artists(me, p()).await);
    assert_rejected("user_charts", dz.user_charts(me, p()).await);
    assert_rejected("user_charts_tracks", dz.user_charts_tracks(me, p()).await);
    assert_rejected("user_charts_albums", dz.user_charts_albums(me, p()).await);
    assert_rejected(
        "user_charts_playlists",
        dz.user_charts_playlists(me, p()).await,
    );
    assert_rejected("user_charts_artists", dz.user_charts_artists(me, p()).await);
    assert_rejected("user_flow", dz.user_flow(me, p()).await);
    assert_rejected("user_followings", dz.user_followings(me, p()).await);
    assert_rejected("user_followers", dz.user_followers(me, p()).await);
    assert_rejected("user_history", dz.user_history(me, p()).await);
    let err = dz.user_notifications(me, p()).await.unwrap_err();
    assert!(matches!(err, Error::Api { code: 600, .. }), "{err}");
    assert_rejected("user_permissions", dz.user_permissions(me).await);
    assert_rejected("user_options", dz.user_options(me).await);
    assert_rejected("user_personal_songs", dz.user_personal_songs(me, p()).await);
    assert_rejected("user_playlists", dz.user_playlists(me, p()).await);
    assert_rejected("user_podcasts", dz.user_podcasts(me, p()).await);
    assert_rejected("user_radios", dz.user_radios(me, p()).await);
    assert_rejected("rec_albums", dz.user_recommendations_albums(me, p()).await);
    assert_rejected(
        "rec_releases",
        dz.user_recommendations_releases(me, p()).await,
    );
    assert_rejected(
        "rec_artists",
        dz.user_recommendations_artists(me, p()).await,
    );
    assert_rejected(
        "rec_playlists",
        dz.user_recommendations_playlists(me, p()).await,
    );
    assert_rejected("rec_tracks", dz.user_recommendations_tracks(me, p()).await);
    assert_rejected("rec_radios", dz.user_recommendations_radios(me, p()).await);
    assert_rejected("user_tracks", dz.user_tracks(me, p()).await);
    assert_rejected("search_history", dz.search_history("a", p()).await);
}

#[test]
fn should_encode_query_when_building_authorize_url() {
    let url = authorize_url(
        "123",
        "http://127.0.0.1:8888/callback",
        &["basic_access", "manage_library"],
        "st ate&x",
    );
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert_eq!(parsed.host_str(), Some("connect.deezer.com"));
    assert_eq!(parsed.path(), "/oauth/auth.php");
    let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(q["app_id"], "123");
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:8888/callback");
    assert_eq!(q["perms"], "basic_access,manage_library");
    assert_eq!(q["state"], "st ate&x");
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback"));
}
