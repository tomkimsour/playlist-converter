use std::{collections::HashSet, process::ExitCode};

use music_api::{auth, deezer::Deezer};
use session::{Session, creds, find, login, read, write};

mod session;

const USAGE: &str = "usage: playlist-converter <playlist-url> --to spotify|deezer|tidal";

type Error = auth::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Service {
    Spotify,
    Deezer,
    Tidal,
}

#[derive(Debug, Clone, PartialEq)]
struct Track {
    isrc: Option<String>,
    title: String,
    artist: String,
}

struct Playlist {
    name: String,
    description: String,
    cover_url: Option<String>,
    tracks: Vec<Track>,
}

type Candidates = Vec<(String, Vec<String>)>;

/// Parses a playlist link into its service and playlist id.
fn parse_url(url: &str) -> Result<(Service, String), Error> {
    let bad = || -> Error {
        format!(
            "unsupported playlist URL {url:?}; expected open.spotify.com/playlist/<id>, spotify:playlist:<id>, \
             deezer.com/[lang/]playlist/<id> or [listen.]tidal.com/[browse/]playlist/<uuid>"
        )
        .into()
    };
    let (service, id) = match url.strip_prefix("spotify:playlist:") {
        Some(id) => (Service::Spotify, id.to_string()),
        None => {
            let full = if url.contains("://") {
                url.to_string()
            } else {
                format!("https://{url}")
            };
            let u = reqwest::Url::parse(&full).map_err(|_| bad())?;
            let service = match u.host_str() {
                Some("open.spotify.com") => Service::Spotify,
                Some("deezer.com" | "www.deezer.com") => Service::Deezer,
                Some("tidal.com" | "www.tidal.com" | "listen.tidal.com") => Service::Tidal,
                _ => return Err(bad()),
            };
            let segments: Vec<&str> = u.path_segments().into_iter().flatten().collect();
            let id = segments
                .iter()
                .position(|s| *s == "playlist")
                .and_then(|i| segments.get(i + 1))
                .ok_or_else(bad)?;
            (service, id.to_string())
        }
    };
    let valid = match service {
        Service::Spotify => !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()),
        Service::Deezer => !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        Service::Tidal => id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
    };
    if valid { Ok((service, id)) } else { Err(bad()) }
}

/// Parses `--to` values such as `spotify`.
fn parse_service(s: &str) -> Option<Service> {
    match s.to_ascii_lowercase().as_str() {
        "spotify" => Some(Service::Spotify),
        "deezer" => Some(Service::Deezer),
        "tidal" => Some(Service::Tidal),
        _ => None,
    }
}

/// Returns the id of the first candidate one of whose artists contains `artist`, ignoring case.
fn pick(artist: &str, candidates: Candidates) -> Option<String> {
    let artist = artist.to_lowercase();
    candidates
        .into_iter()
        .find(|(_, names)| names.iter().any(|n| n.to_lowercase().contains(&artist)))
        .map(|(id, _)| id)
}

/// Looks a track up by ISRC, then by an "artist title" text search filtered with `pick`.
async fn find_with(
    t: &Track,
    by_isrc: impl AsyncFnOnce(String) -> Result<Option<String>, Error>,
    by_text: impl AsyncFnOnce(String) -> Result<Candidates, Error>,
) -> Result<Option<String>, Error> {
    if let Some(isrc) = &t.isrc
        && let Some(id) = by_isrc(isrc.clone()).await?
    {
        return Ok(Some(id));
    }
    Ok(pick(
        &t.artist,
        by_text(format!("{} {}", t.artist, t.title)).await?,
    ))
}

/// Maximum number of tracks one add-to-playlist request accepts.
fn batch_size(s: Service) -> usize {
    match s {
        Service::Spotify | Service::Deezer => 100,
        Service::Tidal => 50,
    }
}

/// Converts the playlist at `url` into a new playlist on `to`.
async fn run(url: &str, to: Service) -> Result<(), Error> {
    let (from, id) = parse_url(url)?;
    let to_creds = creds(to)?;
    let from_creds = match from {
        _ if from == to => None,
        Service::Deezer => None,
        s => Some(creds(s)?),
    };
    let source = match from_creds {
        Some(c) => Some(login(&c).await?),
        None if from != to => Some(Session::Deezer(Deezer::new())),
        None => None,
    };
    let dest = login(&to_creds).await?;
    let mut playlist = read(source.as_ref().unwrap_or(&dest), &id).await?;
    if playlist.name.is_empty() {
        playlist.name = "Converted playlist".into();
    }
    eprintln!(
        "Matching {} tracks of \"{}\"...",
        playlist.tracks.len(),
        playlist.name
    );

    let mut ids = Vec::new();
    let mut unmatched = Vec::new();
    for t in &playlist.tracks {
        match find(&dest, t).await {
            Ok(Some(id)) => ids.push(id),
            Ok(None) => unmatched.push(t),
            Err(e) => {
                eprintln!("note: lookup failed for {} - {}: {e}", t.artist, t.title);
                unmatched.push(t);
            }
        }
    }
    let matched = ids.len();
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));

    let new_url = write(&dest, &playlist, &ids).await?;
    println!("{new_url}");
    println!("matched {matched}/{}", playlist.tracks.len());
    for t in unmatched {
        println!("  unmatched: {} - {}", t.artist, t.title);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (url, to) = match args.as_slice() {
        [url, flag, to] if flag == "--to" => match parse_service(to) {
            Some(to) => (url, to),
            None => {
                eprintln!("unknown service {to:?}\n{USAGE}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(url, to).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "1b4a5d0e-7a2f-4c33-9d1e-0123456789ab";

    #[test]
    fn should_parse_every_accepted_url_form() {
        let ok = [
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
                Service::Spotify,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc123",
                Service::Spotify,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "https://open.spotify.com/intl-fr/playlist/37i9dQZF1DXcBWIGoYBM5M",
                Service::Spotify,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
                Service::Spotify,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "https://www.deezer.com/playlist/11241913244",
                Service::Deezer,
                "11241913244",
            ),
            (
                "https://www.deezer.com/fr/playlist/11241913244",
                Service::Deezer,
                "11241913244",
            ),
            (
                "deezer.com/playlist/11241913244",
                Service::Deezer,
                "11241913244",
            ),
            (
                &format!("https://tidal.com/playlist/{UUID}"),
                Service::Tidal,
                UUID,
            ),
            (
                &format!("https://tidal.com/browse/playlist/{UUID}"),
                Service::Tidal,
                UUID,
            ),
            (
                &format!("https://listen.tidal.com/playlist/{UUID}"),
                Service::Tidal,
                UUID,
            ),
        ];
        for (url, service, id) in ok {
            assert_eq!(parse_url(url).unwrap(), (service, id.to_string()), "{url}");
        }
    }

    #[test]
    fn should_reject_unsupported_urls() {
        for url in [
            "",
            "not a url",
            "https://example.com/playlist/123",
            "https://open.spotify.com/album/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/playlist/",
            "spotify:playlist:",
            "spotify:album:37i9dQZF1DXcBWIGoYBM5M",
            "https://www.deezer.com/playlist/abc",
            "https://tidal.com/browse/playlist/123",
            "https://evil-deezer.com/playlist/123",
        ] {
            assert!(parse_url(url).is_err(), "{url}");
        }
    }

    fn cand(id: &str, artists: &[&str]) -> (String, Vec<String>) {
        (id.into(), artists.iter().map(|a| a.to_string()).collect())
    }

    #[test]
    fn should_pick_first_candidate_whose_artist_contains_source_artist() {
        let c = vec![
            cand("1", &["Someone Else"]),
            cand("2", &["X", "DAFT PUNK"]),
            cand("3", &["Daft Punk"]),
        ];
        assert_eq!(pick("daft punk", c), Some("2".into()));
        assert_eq!(pick("daft punk", vec![cand("1", &["Other"])]), None);
    }

    fn track(isrc: Option<&str>) -> Track {
        Track {
            isrc: isrc.map(String::from),
            title: "One More Time".into(),
            artist: "Daft Punk".into(),
        }
    }

    #[tokio::test]
    async fn should_short_circuit_on_isrc_hit() {
        let found = find_with(
            &track(Some("GBDUW0000059")),
            async |isrc| Ok(Some(format!("id-{isrc}"))),
            async |_| panic!("text search must not run after an ISRC hit"),
        )
        .await
        .unwrap();
        assert_eq!(found, Some("id-GBDUW0000059".into()));
    }

    #[tokio::test]
    async fn should_fall_back_to_text_search_with_artist_check() {
        let miss = async |_: String| Ok(None);
        let found = find_with(&track(Some("X")), miss, async |q| {
            assert_eq!(q, "Daft Punk One More Time");
            Ok(vec![
                cand("cover", &["Tribute Band"]),
                cand("real", &["Daft Punk"]),
            ])
        })
        .await
        .unwrap();
        assert_eq!(found, Some("real".into()));

        let none = find_with(
            &track(None),
            async |_| panic!("no ISRC to look up"),
            async |_| Ok(vec![cand("cover", &["Tribute Band"])]),
        )
        .await
        .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn should_split_ids_into_service_batches() {
        let ids: Vec<u32> = (0..230).collect();
        let sizes = |s| {
            ids.chunks(batch_size(s))
                .map(<[u32]>::len)
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(Service::Spotify), [100, 100, 30]);
        assert_eq!(sizes(Service::Deezer), [100, 100, 30]);
        assert_eq!(sizes(Service::Tidal), [50, 50, 50, 50, 30]);
    }
}
