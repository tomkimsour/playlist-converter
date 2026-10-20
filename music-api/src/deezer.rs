use std::{fmt, time::Duration};

use reqwest::Method;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

pub const BASE_URL: &str = "https://api.deezer.com";
pub const UPLOAD_URL: &str = "https://upload.deezer.com";
const OAUTH_AUTH_URL: &str = "https://connect.deezer.com/oauth/auth.php";
const OAUTH_TOKEN_URL: &str = "https://connect.deezer.com/oauth/access_token.php";
const QUOTA_RETRIES: u32 = 6;
const PAGE_LIMIT: u64 = 100;

#[derive(Debug)]
pub enum Error {
    Http(reqwest::Error),
    Json(serde_json::Error),
    Api {
        code: i64,
        kind: String,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(e) => write!(f, "deezer http error: {e}"),
            Error::Json(e) => write!(f, "deezer decode error: {e}"),
            Error::Api {
                code,
                kind,
                message,
            } => write!(f, "deezer api error {code} ({kind}): {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e.without_url())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn api_error(body: &Value) -> Option<Error> {
    let err = body.get("error").filter(|e| e.is_object())?;
    let text = |k: &str| err[k].as_str().unwrap_or_default().to_string();
    Some(Error::Api {
        code: err["code"].as_i64().unwrap_or(0),
        kind: text("type"),
        message: text("message"),
    })
}

fn ids(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

fn opt<'a>(k: &'a str, v: Option<&str>) -> Option<(&'a str, String)> {
    v.map(|v| (k, v.to_string()))
}

/// Fields of a playlist update; unset fields are left unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaylistUpdate<'a> {
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub public: Option<bool>,
    pub collaborative: Option<bool>,
}

/// Fields of an update to a user's uploaded (personal) track; unset fields are left unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackUpdate<'a> {
    pub title: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
}

/// Login URL of Deezer's server-side OAuth flow, e.g. perms `["basic_access", "manage_library", "offline_access"]`.
pub fn authorize_url(app_id: &str, redirect_uri: &str, perms: &[&str], state: &str) -> String {
    reqwest::Url::parse_with_params(
        OAUTH_AUTH_URL,
        [
            ("app_id", app_id),
            ("redirect_uri", redirect_uri),
            ("perms", &perms.join(",")),
            ("state", state),
        ],
    )
    .expect("static url")
    .into()
}

/// Exchanges the `code` received on the redirect for an access token.
pub async fn exchange_code(app_id: &str, secret: &str, code: &str) -> Result<String> {
    let body: Value = reqwest::Client::new()
        .get(OAUTH_TOKEN_URL)
        .query(&[
            ("app_id", app_id),
            ("secret", secret),
            ("code", code),
            ("output", "json"),
        ])
        .send()
        .await?
        .json()
        .await?;
    match body["access_token"].as_str() {
        Some(t) => Ok(t.to_string()),
        None => Err(api_error(&body).unwrap_or(Error::Api {
            code: 300,
            kind: "OAuthException".into(),
            message: body.to_string(),
        })),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub next: Option<String>,
}

/// Optional `index`/`limit` of a list endpoint; `Paging::default()` uses Deezer's defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct Paging {
    pub index: Option<u64>,
    pub limit: Option<u64>,
}

impl Paging {
    fn query(self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(i) = self.index {
            q.push(("index", i.to_string()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        q
    }
}

/// Search text plus the optional `strict` (disable fuzzy mode) and `order` parameters.
#[derive(Debug, Clone, Copy)]
pub struct SearchQuery<'a> {
    pub q: &'a str,
    pub strict: bool,
    pub order: Option<&'a str>,
}

impl<'a> From<&'a str> for SearchQuery<'a> {
    fn from(q: &'a str) -> Self {
        SearchQuery {
            q,
            strict: false,
            order: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artist {
    pub id: i64,
    pub name: Option<String>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub nb_album: Option<u64>,
    pub nb_fan: Option<u64>,
    pub radio: Option<bool>,
    pub tracklist: Option<String>,
    pub position: Option<u64>,
    pub role: Option<String>,
    pub time_add: Option<i64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Album {
    pub id: i64,
    pub title: Option<String>,
    pub upc: Option<String>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub cover: Option<String>,
    pub cover_small: Option<String>,
    pub cover_medium: Option<String>,
    pub cover_big: Option<String>,
    pub cover_xl: Option<String>,
    pub md5_image: Option<String>,
    pub genre_id: Option<i64>,
    pub genres: Option<Page<Genre>>,
    pub label: Option<String>,
    pub nb_tracks: Option<u64>,
    pub duration: Option<u64>,
    pub fans: Option<u64>,
    pub release_date: Option<String>,
    pub record_type: Option<String>,
    pub available: Option<bool>,
    pub alternative: Option<AlbumAlternative>,
    pub tracklist: Option<String>,
    pub explicit_lyrics: Option<bool>,
    pub explicit_content_lyrics: Option<i64>,
    pub explicit_content_cover: Option<i64>,
    pub contributors: Option<Vec<Artist>>,
    pub artist: Option<Artist>,
    pub tracks: Option<Page<Track>>,
    pub position: Option<u64>,
    pub time_add: Option<i64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// Stub of the other edition of an album.
#[derive(Debug, Clone, Deserialize)]
pub struct AlbumAlternative {
    pub id: i64,
    pub link: Option<String>,
    pub md5_image: Option<String>,
    pub available: Option<bool>,
    pub tracklist: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Track {
    pub id: i64,
    pub readable: Option<bool>,
    pub title: Option<String>,
    pub title_short: Option<String>,
    pub title_version: Option<String>,
    pub unseen: Option<bool>,
    pub isrc: Option<String>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub duration: Option<u64>,
    pub track_position: Option<u64>,
    pub disk_number: Option<u64>,
    pub rank: Option<i64>,
    pub release_date: Option<String>,
    pub explicit_lyrics: Option<bool>,
    pub explicit_content_lyrics: Option<i64>,
    pub explicit_content_cover: Option<i64>,
    pub preview: Option<String>,
    pub bpm: Option<f64>,
    pub gain: Option<f64>,
    pub available_countries: Option<Vec<String>>,
    pub alternative: Option<Box<Track>>,
    pub contributors: Option<Vec<Artist>>,
    pub md5_image: Option<String>,
    pub track_token: Option<String>,
    pub position: Option<u64>,
    pub time_add: Option<i64>,
    pub timestamp: Option<i64>,
    pub artist: Option<Artist>,
    pub album: Option<Album>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub duration: Option<u64>,
    pub public: Option<bool>,
    pub is_loved_track: Option<bool>,
    pub collaborative: Option<bool>,
    pub nb_tracks: Option<u64>,
    pub unseen_track_count: Option<u64>,
    pub fans: Option<u64>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub checksum: Option<String>,
    pub tracklist: Option<String>,
    pub creation_date: Option<String>,
    pub add_date: Option<String>,
    pub mod_date: Option<String>,
    pub md5_image: Option<String>,
    pub picture_type: Option<String>,
    pub time_add: Option<i64>,
    pub time_mod: Option<i64>,
    pub position: Option<u64>,
    pub creator: Option<User>,
    pub user: Option<User>,
    pub tracks: Option<Page<Track>>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: Option<String>,
    pub lastname: Option<String>,
    pub firstname: Option<String>,
    pub email: Option<String>,
    pub status: Option<i64>,
    pub birthday: Option<String>,
    pub inscription_date: Option<String>,
    pub gender: Option<String>,
    pub link: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub country: Option<String>,
    pub lang: Option<String>,
    pub is_kid: Option<bool>,
    pub explicit_content_level: Option<String>,
    pub explicit_content_levels_available: Option<Vec<String>>,
    pub tracklist: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Genre {
    pub id: i64,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Radio {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub share: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub tracklist: Option<String>,
    pub md5_image: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RadioGenre {
    pub id: i64,
    pub title: Option<String>,
    #[serde(default)]
    pub radios: Vec<Radio>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Podcast {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub available: Option<bool>,
    pub fans: Option<u64>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub position: Option<u64>,
    pub time_add: Option<i64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub available: Option<bool>,
    pub release_date: Option<String>,
    pub duration: Option<u64>,
    pub link: Option<String>,
    pub share: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    pub track_token: Option<String>,
    pub podcast: Option<Podcast>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Editorial {
    pub id: i64,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub picture_small: Option<String>,
    pub picture_medium: Option<String>,
    pub picture_big: Option<String>,
    pub picture_xl: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// Result of `/chart/{id}` and `/editorial/{id}/charts`.
#[derive(Debug, Clone, Deserialize)]
pub struct Chart {
    pub tracks: Option<Page<Track>>,
    pub albums: Option<Page<Album>>,
    pub artists: Option<Page<Artist>>,
    pub playlists: Option<Page<Playlist>>,
    pub podcasts: Option<Page<Podcast>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Infos {
    pub country_iso: Option<String>,
    pub country: Option<String>,
    pub open: Option<bool>,
    pub offers: Option<Vec<Value>>,
    pub upload_token: Option<String>,
    pub upload_token_lifetime: Option<u64>,
    pub has_podcasts: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Options {
    pub streaming: Option<bool>,
    pub streaming_duration: Option<i64>,
    pub offline: Option<bool>,
    pub hq: Option<bool>,
    pub ads_display: Option<bool>,
    pub ads_audio: Option<bool>,
    pub too_many_devices: Option<bool>,
    pub can_subscribe: Option<bool>,
    pub radio_skips: Option<i64>,
    pub lossless: Option<bool>,
    pub preview: Option<bool>,
    pub radio: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Oembed {
    pub version: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub cache_age: Option<u64>,
    pub provider_name: Option<String>,
    pub provider_url: Option<String>,
    pub entity: Option<String>,
    pub id: Option<i64>,
    pub url: Option<String>,
    pub author_name: Option<String>,
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_width: Option<u64>,
    pub thumbnail_height: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub html: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Deezer {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl Default for Deezer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deezer {
    pub fn new() -> Self {
        Self::with_base(BASE_URL)
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Deezer {
            http: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Fetches every item of a list endpoint by paging, e.g. `dz.all(|p| dz.playlist_tracks(id, p))`.
    pub async fn all<T, F, Fut>(&self, f: F) -> Result<Vec<T>>
    where
        F: Fn(Paging) -> Fut,
        Fut: std::future::Future<Output = Result<Page<T>>>,
    {
        let mut out = Vec::new();
        let mut index = 0;
        loop {
            let page = f(Paging {
                index: Some(index),
                limit: Some(PAGE_LIMIT),
            })
            .await?;
            let next = page
                .next
                .as_deref()
                .and_then(|n| reqwest::Url::parse(n).ok())
                .and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "index")
                        .and_then(|(_, v)| v.parse::<u64>().ok())
                });
            out.extend(page.data);
            match next {
                Some(i) if i > index => index = i,
                _ => return Ok(out),
            }
        }
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        mut query: Vec<(&str, String)>,
    ) -> Result<Value> {
        if let Some(t) = &self.token {
            query.push(("access_token", t.clone()));
        }
        let url = format!("{}/{}", self.base, path);
        let mut attempt = 0;
        loop {
            let body: Value = self
                .http
                .request(method.clone(), &url)
                .query(&query)
                .send()
                .await?
                .json()
                .await?;
            match api_error(&body) {
                Some(Error::Api { code: 4, .. }) if attempt < QUOTA_RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Some(e) => return Err(e),
                None => return Ok(body),
            }
        }
    }

    async fn get_value(&self, path: &str, query: Vec<(&str, String)>) -> Result<Value> {
        self.send(Method::GET, path, query).await
    }

    async fn act(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Vec<(&str, String)>,
    ) -> Result<()> {
        match self.send(method, path, query).await? {
            Value::Bool(true) => Ok(()),
            other => Err(Error::Api {
                code: 0,
                kind: "UnexpectedResponse".into(),
                message: other.to_string(),
            }),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: Vec<(&str, String)>) -> Result<T> {
        Ok(serde_json::from_value(self.get_value(path, query).await?)?)
    }

    async fn list<T: DeserializeOwned>(
        &self,
        path: &str,
        mut query: Vec<(&str, String)>,
        p: Paging,
    ) -> Result<Page<T>> {
        query.extend(p.query());
        let v = self.get_value(path, query).await?;
        if v == Value::Bool(false) {
            return Ok(serde_json::from_value(json!({"data": []}))?);
        }
        Ok(serde_json::from_value(v)?)
    }

    async fn search_at<T: DeserializeOwned>(
        &self,
        path: &str,
        s: SearchQuery<'_>,
        p: Paging,
    ) -> Result<Page<T>> {
        let mut q = vec![("q", s.q.to_string())];
        if s.strict {
            q.push(("strict", "on".into()));
        }
        if let Some(o) = s.order {
            q.push(("order", o.into()));
        }
        self.list(path, q, p).await
    }

    pub async fn album(&self, id: i64) -> Result<Album> {
        self.get(&format!("album/{id}"), vec![]).await
    }
    pub async fn album_upc(&self, upc: &str) -> Result<Album> {
        self.get(&format!("album/upc:{upc}"), vec![]).await
    }
    pub async fn album_fans(&self, id: i64, p: Paging) -> Result<Page<User>> {
        self.list(&format!("album/{id}/fans"), vec![], p).await
    }
    pub async fn album_tracks(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("album/{id}/tracks"), vec![], p).await
    }

    pub async fn artist(&self, id: i64) -> Result<Artist> {
        self.get(&format!("artist/{id}"), vec![]).await
    }
    pub async fn artist_top(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("artist/{id}/top"), vec![], p).await
    }
    pub async fn artist_albums(&self, id: i64, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("artist/{id}/albums"), vec![], p).await
    }
    pub async fn artist_fans(&self, id: i64, p: Paging) -> Result<Page<User>> {
        self.list(&format!("artist/{id}/fans"), vec![], p).await
    }
    pub async fn artist_related(&self, id: i64, p: Paging) -> Result<Page<Artist>> {
        self.list(&format!("artist/{id}/related"), vec![], p).await
    }
    pub async fn artist_radio(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("artist/{id}/radio"), vec![], p).await
    }
    pub async fn artist_playlists(&self, id: i64, p: Paging) -> Result<Page<Playlist>> {
        self.list(&format!("artist/{id}/playlists"), vec![], p)
            .await
    }

    pub async fn chart(&self, id: i64) -> Result<Chart> {
        self.get(&format!("chart/{id}"), vec![]).await
    }
    pub async fn chart_tracks(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("chart/{id}/tracks"), vec![], p).await
    }
    pub async fn chart_albums(&self, id: i64, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("chart/{id}/albums"), vec![], p).await
    }
    pub async fn chart_artists(&self, id: i64, p: Paging) -> Result<Page<Artist>> {
        self.list(&format!("chart/{id}/artists"), vec![], p).await
    }
    pub async fn chart_playlists(&self, id: i64, p: Paging) -> Result<Page<Playlist>> {
        self.list(&format!("chart/{id}/playlists"), vec![], p).await
    }
    pub async fn chart_podcasts(&self, id: i64, p: Paging) -> Result<Page<Podcast>> {
        self.list(&format!("chart/{id}/podcasts"), vec![], p).await
    }

    pub async fn editorials(&self, p: Paging) -> Result<Page<Editorial>> {
        self.list("editorial", vec![], p).await
    }
    pub async fn editorial(&self, id: i64) -> Result<Editorial> {
        self.get(&format!("editorial/{id}"), vec![]).await
    }
    pub async fn editorial_selection(
        &self,
        id: i64,
        date: Option<&str>,
        p: Paging,
    ) -> Result<Page<Album>> {
        let q = date.map(|d| ("date", d.to_string())).into_iter().collect();
        self.list(&format!("editorial/{id}/selection"), q, p).await
    }
    pub async fn editorial_charts(&self, id: i64) -> Result<Chart> {
        self.get(&format!("editorial/{id}/charts"), vec![]).await
    }
    pub async fn editorial_releases(&self, id: i64, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("editorial/{id}/releases"), vec![], p)
            .await
    }

    pub async fn episode(&self, id: i64) -> Result<Episode> {
        self.get(&format!("episode/{id}"), vec![]).await
    }

    pub async fn genres(&self, p: Paging) -> Result<Page<Genre>> {
        self.list("genre", vec![], p).await
    }
    pub async fn genre(&self, id: i64) -> Result<Genre> {
        self.get(&format!("genre/{id}"), vec![]).await
    }
    pub async fn genre_artists(&self, id: i64, p: Paging) -> Result<Page<Artist>> {
        self.list(&format!("genre/{id}/artists"), vec![], p).await
    }
    pub async fn genre_podcasts(&self, id: i64, p: Paging) -> Result<Page<Podcast>> {
        self.list(&format!("genre/{id}/podcasts"), vec![], p).await
    }
    pub async fn genre_radios(&self, id: i64, p: Paging) -> Result<Page<Radio>> {
        self.list(&format!("genre/{id}/radios"), vec![], p).await
    }

    pub async fn infos(&self) -> Result<Infos> {
        self.get("infos", vec![]).await
    }

    pub async fn oembed(&self, url: &str) -> Result<Oembed> {
        self.get("oembed", vec![("url", url.to_string())]).await
    }

    pub async fn playlist(&self, id: i64) -> Result<Playlist> {
        self.get(&format!("playlist/{id}"), vec![]).await
    }
    pub async fn playlist_fans(&self, id: i64, p: Paging) -> Result<Page<User>> {
        self.list(&format!("playlist/{id}/fans"), vec![], p).await
    }
    pub async fn playlist_tracks(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("playlist/{id}/tracks"), vec![], p).await
    }
    pub async fn playlist_radio(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("playlist/{id}/radio"), vec![], p).await
    }

    pub async fn podcast(&self, id: i64) -> Result<Podcast> {
        self.get(&format!("podcast/{id}"), vec![]).await
    }
    pub async fn podcast_episodes(&self, id: i64, p: Paging) -> Result<Page<Episode>> {
        self.list(&format!("podcast/{id}/episodes"), vec![], p)
            .await
    }

    pub async fn radios(&self, p: Paging) -> Result<Page<Radio>> {
        self.list("radio", vec![], p).await
    }
    pub async fn radio(&self, id: i64) -> Result<Radio> {
        self.get(&format!("radio/{id}"), vec![]).await
    }
    pub async fn radio_genres(&self, p: Paging) -> Result<Page<RadioGenre>> {
        self.list("radio/genres", vec![], p).await
    }
    pub async fn radio_top(&self, p: Paging) -> Result<Page<Radio>> {
        self.list("radio/top", vec![], p).await
    }
    pub async fn radio_tracks(&self, id: i64, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("radio/{id}/tracks"), vec![], p).await
    }
    pub async fn radio_lists(&self, p: Paging) -> Result<Page<Radio>> {
        self.list("radio/lists", vec![], p).await
    }

    pub async fn search<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Track>> {
        self.search_at("search", s.into(), p).await
    }
    pub async fn search_album<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Album>> {
        self.search_at("search/album", s.into(), p).await
    }
    pub async fn search_artist<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Artist>> {
        self.search_at("search/artist", s.into(), p).await
    }
    pub async fn search_history<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Value>> {
        self.search_at("search/history", s.into(), p).await
    }
    pub async fn search_playlist<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Playlist>> {
        self.search_at("search/playlist", s.into(), p).await
    }
    pub async fn search_podcast<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Podcast>> {
        self.search_at("search/podcast", s.into(), p).await
    }
    pub async fn search_radio<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Radio>> {
        self.search_at("search/radio", s.into(), p).await
    }
    pub async fn search_track<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<Track>> {
        self.search_at("search/track", s.into(), p).await
    }
    pub async fn search_user<'a>(
        &self,
        s: impl Into<SearchQuery<'a>>,
        p: Paging,
    ) -> Result<Page<User>> {
        self.search_at("search/user", s.into(), p).await
    }

    pub async fn track(&self, id: i64) -> Result<Track> {
        self.get(&format!("track/{id}"), vec![]).await
    }
    pub async fn track_isrc(&self, isrc: &str) -> Result<Track> {
        self.get(&format!("track/isrc:{isrc}"), vec![]).await
    }

    pub async fn user(&self, user: &str) -> Result<User> {
        self.get(&format!("user/{user}"), vec![]).await
    }
    pub async fn user_albums(&self, user: &str, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("user/{user}/albums"), vec![], p).await
    }
    pub async fn user_artists(&self, user: &str, p: Paging) -> Result<Page<Artist>> {
        self.list(&format!("user/{user}/artists"), vec![], p).await
    }
    pub async fn user_charts(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/charts"), vec![], p).await
    }
    pub async fn user_charts_tracks(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/charts/tracks"), vec![], p)
            .await
    }
    pub async fn user_charts_albums(&self, user: &str, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("user/{user}/charts/albums"), vec![], p)
            .await
    }
    pub async fn user_charts_playlists(&self, user: &str, p: Paging) -> Result<Page<Playlist>> {
        self.list(&format!("user/{user}/charts/playlists"), vec![], p)
            .await
    }
    pub async fn user_charts_artists(&self, user: &str, p: Paging) -> Result<Page<Artist>> {
        self.list(&format!("user/{user}/charts/artists"), vec![], p)
            .await
    }
    pub async fn user_flow(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/flow"), vec![], p).await
    }
    pub async fn user_followings(&self, user: &str, p: Paging) -> Result<Page<User>> {
        self.list(&format!("user/{user}/followings"), vec![], p)
            .await
    }
    pub async fn user_followers(&self, user: &str, p: Paging) -> Result<Page<User>> {
        self.list(&format!("user/{user}/followers"), vec![], p)
            .await
    }
    pub async fn user_history(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/history"), vec![], p).await
    }
    pub async fn user_notifications(&self, user: &str, p: Paging) -> Result<Page<Value>> {
        self.list(&format!("user/{user}/notifications"), vec![], p)
            .await
    }
    pub async fn user_permissions(&self, user: &str) -> Result<Value> {
        self.get(&format!("user/{user}/permissions"), vec![]).await
    }
    pub async fn user_options(&self, user: &str) -> Result<Options> {
        self.get(&format!("user/{user}/options"), vec![]).await
    }
    pub async fn user_personal_songs(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/personal_songs"), vec![], p)
            .await
    }
    pub async fn user_playlists(&self, user: &str, p: Paging) -> Result<Page<Playlist>> {
        self.list(&format!("user/{user}/playlists"), vec![], p)
            .await
    }
    pub async fn user_podcasts(&self, user: &str, p: Paging) -> Result<Page<Podcast>> {
        self.list(&format!("user/{user}/podcasts"), vec![], p).await
    }
    pub async fn user_radios(&self, user: &str, p: Paging) -> Result<Page<Radio>> {
        self.list(&format!("user/{user}/radios"), vec![], p).await
    }
    pub async fn user_recommendations_albums(&self, user: &str, p: Paging) -> Result<Page<Album>> {
        self.list(&format!("user/{user}/recommendations/albums"), vec![], p)
            .await
    }
    pub async fn user_recommendations_releases(
        &self,
        user: &str,
        p: Paging,
    ) -> Result<Page<Album>> {
        self.list(&format!("user/{user}/recommendations/releases"), vec![], p)
            .await
    }
    pub async fn user_recommendations_artists(
        &self,
        user: &str,
        p: Paging,
    ) -> Result<Page<Artist>> {
        self.list(&format!("user/{user}/recommendations/artists"), vec![], p)
            .await
    }
    pub async fn user_recommendations_playlists(
        &self,
        user: &str,
        p: Paging,
    ) -> Result<Page<Playlist>> {
        self.list(&format!("user/{user}/recommendations/playlists"), vec![], p)
            .await
    }
    pub async fn user_recommendations_tracks(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/recommendations/tracks"), vec![], p)
            .await
    }
    pub async fn user_recommendations_radios(&self, user: &str, p: Paging) -> Result<Page<Radio>> {
        self.list(&format!("user/{user}/recommendations/radios"), vec![], p)
            .await
    }
    pub async fn user_tracks(&self, user: &str, p: Paging) -> Result<Page<Track>> {
        self.list(&format!("user/{user}/tracks"), vec![], p).await
    }

    pub async fn episode_bookmark_set(&self, id: i64, offset: u64) -> Result<()> {
        let q = vec![("offset", offset.to_string())];
        self.act(Method::POST, &format!("episode/{id}/bookmark"), q)
            .await
    }
    pub async fn episode_bookmark_remove(&self, id: i64) -> Result<()> {
        self.act(Method::DELETE, &format!("episode/{id}/bookmark"), vec![])
            .await
    }

    pub async fn playlist_update(&self, id: i64, u: &PlaylistUpdate<'_>) -> Result<()> {
        let q = [
            opt("title", u.title),
            opt("description", u.description),
            opt("public", u.public.map(|b| b.to_string()).as_deref()),
            opt(
                "collaborative",
                u.collaborative.map(|b| b.to_string()).as_deref(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        self.act(Method::POST, &format!("playlist/{id}"), q).await
    }
    pub async fn playlist_delete(&self, id: i64) -> Result<()> {
        self.act(Method::DELETE, &format!("playlist/{id}"), vec![])
            .await
    }
    pub async fn playlist_seen(&self, id: i64) -> Result<()> {
        self.act(Method::POST, &format!("playlist/{id}/seen"), vec![])
            .await
    }
    pub async fn playlist_add_tracks(&self, id: i64, tracks: &[i64]) -> Result<()> {
        let q = vec![("songs", ids(tracks))];
        self.act(Method::POST, &format!("playlist/{id}/tracks"), q)
            .await
    }
    pub async fn playlist_reorder_tracks(&self, id: i64, order: &[i64]) -> Result<()> {
        let q = vec![("order", ids(order))];
        self.act(Method::POST, &format!("playlist/{id}/tracks"), q)
            .await
    }
    pub async fn playlist_remove_tracks(&self, id: i64, tracks: &[i64]) -> Result<()> {
        let q = vec![("songs", ids(tracks))];
        self.act(Method::DELETE, &format!("playlist/{id}/tracks"), q)
            .await
    }

    pub async fn track_update(&self, id: i64, u: &TrackUpdate<'_>) -> Result<()> {
        let q = [
            opt("title", u.title),
            opt("artist", u.artist),
            opt("album", u.album),
        ]
        .into_iter()
        .flatten()
        .collect();
        self.act(Method::POST, &format!("track/{id}"), q).await
    }
    pub async fn track_delete(&self, id: i64) -> Result<()> {
        self.act(Method::DELETE, &format!("track/{id}"), vec![])
            .await
    }

    pub async fn user_add_albums(&self, user: &str, albums: &[i64]) -> Result<()> {
        let q = vec![("album_ids", ids(albums))];
        self.act(Method::POST, &format!("user/{user}/albums"), q)
            .await
    }
    pub async fn user_remove_album(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("album_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/albums"), q)
            .await
    }
    pub async fn user_add_artists(&self, user: &str, artists: &[i64]) -> Result<()> {
        let q = vec![("artist_ids", ids(artists))];
        self.act(Method::POST, &format!("user/{user}/artists"), q)
            .await
    }
    pub async fn user_remove_artist(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("artist_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/artists"), q)
            .await
    }
    pub async fn user_add_playlists(&self, user: &str, playlists: &[i64]) -> Result<()> {
        let q = vec![("playlist_ids", ids(playlists))];
        self.act(Method::POST, &format!("user/{user}/playlists"), q)
            .await
    }
    pub async fn user_remove_playlist(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("playlist_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/playlists"), q)
            .await
    }
    pub async fn user_add_podcast(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("podcast_id", id.to_string())];
        self.act(Method::POST, &format!("user/{user}/podcasts"), q)
            .await
    }
    pub async fn user_remove_podcast(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("podcast_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/podcasts"), q)
            .await
    }
    pub async fn user_add_radio(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("radio_id", id.to_string())];
        self.act(Method::POST, &format!("user/{user}/radios"), q)
            .await
    }
    pub async fn user_remove_radio(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("radio_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/radios"), q)
            .await
    }
    pub async fn user_add_tracks(&self, user: &str, tracks: &[i64]) -> Result<()> {
        let q = vec![("track_ids", ids(tracks))];
        self.act(Method::POST, &format!("user/{user}/tracks"), q)
            .await
    }
    pub async fn user_remove_track(&self, user: &str, id: i64) -> Result<()> {
        let q = vec![("track_id", id.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/tracks"), q)
            .await
    }
    pub async fn user_follow(&self, user: &str, target: i64) -> Result<()> {
        let q = vec![("user_id", target.to_string())];
        self.act(Method::POST, &format!("user/{user}/followings"), q)
            .await
    }
    pub async fn user_unfollow(&self, user: &str, target: i64) -> Result<()> {
        let q = vec![("user_id", target.to_string())];
        self.act(Method::DELETE, &format!("user/{user}/followings"), q)
            .await
    }
    pub async fn user_notify(&self, user: &str, message: &str) -> Result<()> {
        let q = vec![("message", message.to_string())];
        self.act(Method::POST, &format!("user/{user}/notifications"), q)
            .await
    }

    /// Creates a playlist and returns its id.
    pub async fn create_playlist(&self, user: &str, title: &str) -> Result<i64> {
        let q = vec![("title", title.to_string())];
        let v = self
            .send(Method::POST, &format!("user/{user}/playlists"), q)
            .await?;
        v["id"].as_i64().ok_or_else(|| Error::Api {
            code: 0,
            kind: "UnexpectedResponse".into(),
            message: v.to_string(),
        })
    }

    /// Replaces a playlist's cover with a JPEG; needs a token whose `/infos` carries an upload token.
    pub async fn upload_cover(&self, playlist: i64, jpeg: Vec<u8>) -> Result<()> {
        let missing = |message: &str| Error::Api {
            code: 300,
            kind: "OAuthException".into(),
            message: message.into(),
        };
        let token = self
            .token
            .clone()
            .ok_or_else(|| missing("no access token"))?;
        let upload_token = self
            .infos()
            .await?
            .upload_token
            .ok_or_else(|| missing("no upload token"))?;
        let file = reqwest::multipart::Part::bytes(jpeg)
            .file_name("cover.jpg")
            .mime_str("image/jpeg")?;
        let form = reqwest::multipart::Form::new()
            .text("access_token", token)
            .text("upload_token", upload_token)
            .part("file", file);
        let body: Value = self
            .http
            .post(format!("{UPLOAD_URL}/playlist/{playlist}"))
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;
        api_error(&body).map_or(Ok(()), Err)
    }
}
