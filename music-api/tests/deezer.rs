use music_api::deezer::{Deezer, Error, Paging};

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
