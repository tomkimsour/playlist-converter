use std::{num::NonZeroU64, time::Duration};

use music_api::{
    auth,
    deezer::{self, Deezer, Paging, PlaylistUpdate},
    spotify, tidal,
};
use serde_json::{Value, json};

use crate::{Candidates, Error, Playlist, Service, Track, batch_size, find_with};

const SPOTIFY_AUTHORIZE: &str = "https://accounts.spotify.com/authorize";
const SPOTIFY_TOKEN: &str = "https://accounts.spotify.com/api/token";
const SPOTIFY_SCOPES: &str = "playlist-read-private playlist-read-collaborative playlist-modify-public playlist-modify-private ugc-image-upload";
const TIDAL_AUTHORIZE: &str = "https://login.tidal.com/authorize";
const TIDAL_TOKEN: &str = "https://auth.tidal.com/v1/oauth2/token";
const TIDAL_SCOPES: &str = "user.read playlists.read playlists.write search.read";
const SPOTIFY_COVER_MAX_BASE64: usize = 256 * 1024;
const MAX_ATTEMPTS: u32 = 5;

pub enum Creds {
    Spotify(String),
    Tidal(String),
    Deezer(String, String),
}

pub enum Session {
    Spotify(spotify::Client),
    Tidal(tidal::Client, String),
    Deezer(Deezer),
}

/// Runs a generated-client call, retrying on HTTP 429 after `Retry-After` seconds.
async fn retry<T, F, Fut>(mut call: F) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<spotify::ResponseValue<T>, spotify::Error<()>>>,
{
    for _ in 0..MAX_ATTEMPTS {
        match call().await {
            Ok(v) => return Ok(v.into_inner()),
            Err(spotify::Error::UnexpectedResponse(r)) if r.status() == 429 => {
                let secs = r
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()?.parse().ok())
                    .unwrap_or(5);
                eprintln!("rate limited, retrying in {secs}s");
                tokio::time::sleep(Duration::from_secs(secs)).await;
            }
            Err(spotify::Error::UnexpectedResponse(r)) => {
                let (url, status) = (r.url().clone(), r.status());
                return Err(
                    format!("{url}: {status}: {}", r.text().await.unwrap_or_default()).into(),
                );
            }
            Err(e) => return Err(e.to_string().into()),
        }
    }
    Err(format!("still rate limited after {MAX_ATTEMPTS} attempts").into())
}

/// Reads a non-empty environment variable.
fn env(name: &str) -> Result<String, Error> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("missing environment variable {name}").into())
}

/// Collects the credentials needed to log in to `s` from the environment.
pub fn creds(s: Service) -> Result<Creds, Error> {
    Ok(match s {
        Service::Spotify => Creds::Spotify(env("SPOTIFY_CLIENT_ID")?),
        Service::Tidal => Creds::Tidal(env("TIDAL_CLIENT_ID")?),
        Service::Deezer => Creds::Deezer(env("DEEZER_APP_ID")?, env("DEEZER_SECRET")?),
    })
}

/// Runs the browser PKCE login and returns the access token.
async fn pkce_login(
    name: &str,
    authorize: &str,
    token_url: &str,
    client_id: &str,
    scope: &str,
) -> Result<String, Error> {
    let pkce = auth::Pkce::new();
    let state = auth::random_state();
    let url = reqwest::Url::parse_with_params(
        authorize,
        [
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", auth::REDIRECT_URI),
            ("code_challenge_method", "S256"),
            ("code_challenge", &pkce.challenge),
            ("state", &state),
            ("scope", scope),
        ],
    )?;
    eprintln!("Log in to {name} by opening:\n  {url}");
    let code = auth::wait_for_code(&state).await?;
    let form = [
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", auth::REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", &pkce.verifier),
    ];
    Ok(auth::exchange(&reqwest::Client::new(), token_url, &form)
        .await?
        .access_token)
}

/// Logs the user in to a service.
pub async fn login(creds: &Creds) -> Result<Session, Error> {
    Ok(match creds {
        Creds::Spotify(id) => {
            let token = pkce_login(
                "Spotify",
                SPOTIFY_AUTHORIZE,
                SPOTIFY_TOKEN,
                id,
                SPOTIFY_SCOPES,
            )
            .await?;
            Session::Spotify(spotify::client(spotify::BASE_URL, &token))
        }
        Creds::Tidal(id) => {
            let token = pkce_login("TIDAL", TIDAL_AUTHORIZE, TIDAL_TOKEN, id, TIDAL_SCOPES).await?;
            let c = tidal::client(tidal::BASE_URL, &token);
            let me = retry(|| c.get_users_id("me", None, None, None)).await?;
            let country = me
                .data
                .attributes
                .ok_or("TIDAL user without attributes")?
                .country;
            Session::Tidal(c, country)
        }
        Creds::Deezer(app_id, secret) => {
            let state = auth::random_state();
            let perms = ["basic_access", "manage_library", "offline_access"];
            let url = deezer::authorize_url(app_id, auth::REDIRECT_URI, &perms, &state);
            eprintln!("Log in to Deezer by opening:\n  {url}");
            let code = auth::wait_for_code(&state).await?;
            Session::Deezer(
                Deezer::new().with_token(deezer::exchange_code(app_id, secret, &code).await?),
            )
        }
    })
}

/// Builds a `Track` from a JSON:API `included` array: the `tracks` resource `id` and its first included artist.
fn tidal_track(id: &str, included: &[Value]) -> Option<Track> {
    let find = |kind: &str, id: &Value| {
        included
            .iter()
            .find(|v| v["type"] == kind && &v["id"] == id)
    };
    let t = find("tracks", &json!(id))?;
    let artist = t["relationships"]["artists"]["data"]
        .get(0)
        .and_then(|a| find("artists", &a["id"]))
        .and_then(|a| a["attributes"]["name"].as_str())
        .unwrap_or_default();
    Some(Track {
        isrc: t["attributes"]["isrc"].as_str().map(String::from),
        title: t["attributes"]["title"].as_str()?.to_string(),
        artist: artist.to_string(),
    })
}

/// Picks the URL of the included artwork file closest to 640 px wide.
fn tidal_cover(included: &[Value]) -> Option<String> {
    let art = included.iter().find(|v| v["type"] == "artworks")?;
    art["attributes"]["files"]
        .as_array()?
        .iter()
        .min_by_key(|f| (f["meta"]["width"].as_i64().unwrap_or(0) - 640).abs())?["href"]
        .as_str()
        .map(String::from)
}

/// Body of `POST /playlists` creating an unlisted TIDAL playlist.
fn tidal_create_body(
    name: &str,
    description: &str,
) -> Result<tidal::types::PlaylistsCreateOperationPayload, Error> {
    let name: String = name.chars().take(250).collect();
    let description = (!description.is_empty()).then_some(description);
    Ok(serde_json::from_value(json!({
        "data": {"type": "playlists", "attributes": {"name": name, "description": description, "accessType": "UNLISTED"}}
    }))?)
}

/// Body of `POST /playlists/{id}/relationships/items` adding tracks by id.
fn tidal_items_body(
    ids: &[String],
) -> Result<tidal::types::PlaylistsItemsRelationshipAddOperationPayload, Error> {
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "type": "tracks"}))
        .collect();
    Ok(serde_json::from_value(json!({ "data": data }))?)
}

/// Reads a playlist and its tracks.
pub async fn read(session: &Session, id: &str) -> Result<Playlist, Error> {
    match session {
        Session::Spotify(c) => {
            let p = retry(|| c.get_playlist(id, None, None, None)).await?;
            let mut tracks = Vec::new();
            let mut offset = 0;
            loop {
                let page = retry(|| {
                    c.get_playlists_items(id, None, None, NonZeroU64::new(50), None, Some(offset))
                })
                .await?;
                offset += page.items.len() as i64;
                let done = page.items.is_empty() || page.next.is_none();
                tracks.extend(page.items.into_iter().filter_map(|e| {
                    match e.item? {
                        spotify::types::PlaylistTrackObjectItem::TrackObject(t) => Some(Track {
                            isrc: t.external_ids.and_then(|e| e.isrc),
                            title: t.name.unwrap_or_default(),
                            artist: t
                                .artists
                                .into_iter()
                                .next()
                                .and_then(|a| a.name)
                                .unwrap_or_default(),
                        }),
                        _ => None,
                    }
                }));
                if done {
                    break;
                }
            }
            Ok(Playlist {
                name: p.name.unwrap_or_default(),
                description: p.description.unwrap_or_default(),
                cover_url: p.images.into_iter().next().map(|i| i.url),
                tracks,
            })
        }
        Session::Deezer(dz) => {
            let pid: i64 = id.parse()?;
            let p = dz.playlist(pid).await?;
            let tracks = dz.all(|pg| dz.playlist_tracks(pid, pg)).await?;
            Ok(Playlist {
                name: p.title.unwrap_or_default(),
                description: p.description.unwrap_or_default(),
                cover_url: p.picture_big,
                tracks: tracks
                    .into_iter()
                    .map(|t| Track {
                        isrc: t.isrc,
                        title: t.title.unwrap_or_default(),
                        artist: t.artist.and_then(|a| a.name).unwrap_or_default(),
                    })
                    .collect(),
            })
        }
        Session::Tidal(c, cc) => {
            let include = vec!["coverArt".to_string()];
            let p = retry(|| c.get_playlists_id(id, Some(cc), Some(&include), None, None)).await?;
            let attrs = p.data.attributes;
            let include = vec!["items".to_string(), "items.artists".to_string()];
            let mut tracks = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let page = retry(|| {
                    c.get_playlists_id_relationships_items(
                        id,
                        Some(cc),
                        None,
                        Some(&include),
                        cursor.as_deref(),
                        None,
                        None,
                    )
                })
                .await?;
                let included = page.included.map(|i| i.0).unwrap_or_default();
                tracks.extend(
                    page.data
                        .iter()
                        .filter(|d| d.type_ == "tracks")
                        .filter_map(|d| tidal_track(&d.id, &included)),
                );
                let next = page.links.meta.map(|m| m.next_cursor);
                if next.is_none() || next == cursor {
                    break;
                }
                cursor = next;
            }
            Ok(Playlist {
                name: attrs
                    .as_ref()
                    .map(|a| a.name.to_string())
                    .unwrap_or_default(),
                description: attrs
                    .as_ref()
                    .and_then(|a| a.description.as_ref())
                    .map(|d| d.to_string())
                    .unwrap_or_default(),
                cover_url: p.included.and_then(|i| tidal_cover(&i.0)),
                tracks,
            })
        }
    }
}

/// Text-searches Spotify tracks, returning (uri, artist names) pairs.
async fn spotify_search(c: &spotify::Client, q: &str) -> Result<Candidates, Error> {
    let types = vec![spotify::types::ItemTypeItem::Track];
    let r = retry(|| {
        c.search(
            None,
            NonZeroU64::new(10),
            Some("from_token"),
            None,
            q,
            &types,
        )
    })
    .await?;
    Ok(r.tracks
        .map(|p| p.items)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            Some((
                t.uri?,
                t.artists.into_iter().filter_map(|a| a.name).collect(),
            ))
        })
        .collect())
}

/// Finds the destination id (Spotify URI, Deezer or TIDAL track id) of a track.
pub async fn find(session: &Session, t: &Track) -> Result<Option<String>, Error> {
    match session {
        Session::Spotify(c) => {
            find_with(
                t,
                async |isrc| {
                    Ok(spotify_search(c, &format!("isrc:{isrc}"))
                        .await?
                        .into_iter()
                        .next()
                        .map(|(uri, _)| uri))
                },
                async |q| spotify_search(c, &q).await,
            )
            .await
        }
        Session::Deezer(dz) => {
            find_with(
                t,
                async |isrc| match dz.track_isrc(&isrc).await {
                    Ok(track) => Ok(Some(track.id.to_string())),
                    Err(deezer::Error::Api { code: 800, .. }) => Ok(None),
                    Err(e) => Err(e.into()),
                },
                async |q| {
                    let paging = Paging {
                        index: None,
                        limit: Some(10),
                    };
                    let page = dz.search_track(q.as_str(), paging).await?;
                    Ok(page
                        .data
                        .into_iter()
                        .map(|t| {
                            (
                                t.id.to_string(),
                                t.artist.and_then(|a| a.name).into_iter().collect(),
                            )
                        })
                        .collect())
                },
            )
            .await
        }
        Session::Tidal(c, cc) => {
            find_with(
                t,
                async |isrc| {
                    let filter = vec![isrc.parse()?];
                    let r = retry(|| {
                        c.get_tracks(
                            Some(cc),
                            None,
                            Some(&filter),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        )
                    })
                    .await?;
                    Ok(r.data.into_iter().next().map(|t| t.id))
                },
                async |q| {
                    let query = q.chars().take(256).collect::<String>().parse()?;
                    let include = vec!["tracks".to_string(), "tracks.artists".to_string()];
                    let r = retry(|| {
                        c.get_search_results(
                            None,
                            Some(cc),
                            None,
                            None,
                            &query,
                            Some(&include),
                            None,
                            None,
                            None,
                        )
                    })
                    .await?;
                    let included = r.included.map(|i| i.0).unwrap_or_default();
                    let ids = r
                        .data
                        .into_iter()
                        .next()
                        .and_then(|d| d.relationships?.tracks)
                        .map(|t| t.data)
                        .unwrap_or_default();
                    Ok(ids
                        .into_iter()
                        .filter_map(|d| {
                            Some((d.id.clone(), vec![tidal_track(&d.id, &included)?.artist]))
                        })
                        .collect())
                },
            )
            .await
        }
    }
}

/// Downloads a cover image.
async fn fetch(url: &str) -> Result<Vec<u8>, Error> {
    Ok(reqwest::get(url)
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

/// Creates the playlist, adds the tracks, copies the description and cover, and returns the new playlist's URL.
pub async fn write(session: &Session, p: &Playlist, ids: &[String]) -> Result<String, Error> {
    let cover = match &p.cover_url {
        Some(url) => fetch(url)
            .await
            .inspect_err(|e| eprintln!("note: cover not copied: {e}"))
            .ok(),
        None => None,
    };
    match session {
        Session::Spotify(c) => {
            let body = spotify::types::CreatePlaylistBody {
                name: p.name.clone(),
                description: Some(p.description.replace(['\n', '\r'], " "))
                    .filter(|d| !d.is_empty()),
                public: Some(false),
                collaborative: None,
            };
            let created = retry(|| c.create_playlist(&body)).await?;
            let pid = created
                .id
                .ok_or("Spotify returned a playlist without an id")?;
            for chunk in ids.chunks(batch_size(Service::Spotify)) {
                let body = spotify::types::AddItemsToPlaylistBody {
                    position: None,
                    uris: chunk.to_vec(),
                };
                retry(|| c.add_items_to_playlist(&pid, None, None, &body)).await?;
            }
            match cover {
                Some(jpeg) if jpeg.len().div_ceil(3) * 4 > SPOTIFY_COVER_MAX_BASE64 => {
                    eprintln!("note: cover not copied: larger than Spotify's 256 KB limit")
                }
                Some(jpeg) => {
                    if let Err(e) = spotify::upload_playlist_cover(c, &pid, &jpeg).await {
                        eprintln!("note: cover not copied: {e}");
                    }
                }
                None => {}
            }
            Ok(created
                .external_urls
                .and_then(|u| u.spotify)
                .unwrap_or_else(|| format!("https://open.spotify.com/playlist/{pid}")))
        }
        Session::Deezer(dz) => {
            let pid = dz.create_playlist("me", &p.name).await?;
            for chunk in ids.chunks(batch_size(Service::Deezer)) {
                let tracks = chunk
                    .iter()
                    .map(|id| id.parse())
                    .collect::<Result<Vec<i64>, _>>()?;
                dz.playlist_add_tracks(pid, &tracks).await?;
            }
            if !p.description.is_empty() {
                let update = PlaylistUpdate {
                    description: Some(&p.description),
                    ..Default::default()
                };
                if let Err(e) = dz.playlist_update(pid, &update).await {
                    eprintln!("note: description not copied: {e}");
                }
            }
            if let Some(jpeg) = cover
                && let Err(e) = dz.upload_cover(pid, jpeg).await
            {
                eprintln!("note: cover not copied: {e}");
            }
            Ok(format!("https://www.deezer.com/playlist/{pid}"))
        }
        Session::Tidal(c, _) => {
            let body = tidal_create_body(&p.name, &p.description)?;
            let pid = retry(|| c.post_playlists(None, &body)).await?.data.id;
            for chunk in ids.chunks(batch_size(Service::Tidal)) {
                let body = tidal_items_body(chunk)?;
                retry(|| c.post_playlists_id_relationships_items(&pid, None, &body)).await?;
            }
            if cover.is_some() {
                eprintln!("note: cover not copied: TIDAL's API cannot set playlist covers");
            }
            Ok(format!("https://tidal.com/playlist/{pid}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_resolve_tidal_track_and_artist_from_included() {
        let included = vec![
            json!({"type": "artists", "id": "7", "attributes": {"name": "Daft Punk"}}),
            json!({"type": "tracks", "id": "42", "attributes": {"title": "One More Time", "isrc": "GBDUW0000059"},
                   "relationships": {"artists": {"data": [{"type": "artists", "id": "7"}]}}}),
        ];
        assert_eq!(
            tidal_track("42", &included),
            Some(Track {
                isrc: Some("GBDUW0000059".into()),
                title: "One More Time".into(),
                artist: "Daft Punk".into(),
            })
        );
        assert_eq!(tidal_track("43", &included), None);
    }

    #[test]
    fn should_build_valid_tidal_write_payloads() {
        let create = tidal_create_body(&"n".repeat(300), "").unwrap();
        assert_eq!(create.data.attributes.name.chars().count(), 250);
        assert!(create.data.attributes.description.is_none());
        let ids: Vec<String> = ["1", "2"].map(String::from).to_vec();
        assert_eq!(tidal_items_body(&ids).unwrap().data.len(), 2);
    }
}
