# Plan: `music-api` crate (Spotify + TIDAL + Deezer) and an any-to-any playlist converter

## Context

- The repo does not build from a fresh clone. `deezer-rs/` and `rspotify/` are submodule gitlinks with no `.gitmodules`, pointing at the
  forks `tomkimsour/deezer-rs@00c0b9d` and `tomkimsour/rspotify@b6370ec` (open upstream PR ramsayleung/rspotify#439).
- The rspotify fork is 47 commits behind and calls endpoints Spotify removed in Feb 2026
  (`/playlists/{id}/tracks` → `/items`, `POST /users/{id}/playlists` → `POST /me/playlists`).
- User decisions:
  - One workspace crate `music-api` with **full spec coverage**: Spotify and TIDAL generated from their OpenAPI specs, Deezer hand-written (no spec).
  - Tested **without any credentials**.
  - The converter is rebuilt on top of the crate and works between any pair of services, **including TIDAL**. You accept the
    TIDAL guideline risk: their §III forbids transfer to or from another service without written approval.

## Facts the design rests on (researched 2026-10-20)

| | Spotify | TIDAL | Deezer |
|---|---|---|---|
| Spec | Official OAS 3.0.3. Vendor sonallux `fixed-spotify-open-api.yml`: a daily-synced copy of the official file plus generator fixes | Official OAS 3.0.1 v1.10.193 (2 MB). Changes almost daily, so pin it | No machine-readable spec. The official reference is behind a login and was exported from your session on 2026-10-20 (91 pages; see the Appendix) |
| Size | 97 operations, 48 deprecated (the removed endpoints) | 422 operations, 11 deprecated, 1346 schemas | About 110 endpoints: 14 objects, about 60 GET connections, 17 POST + 12 DELETE actions, 5 `add-with-options` |
| progenitor blockers | The cover-upload body is `image/jpeg` | **No operationIds**; every body is `application/vnd.api+json` | n/a |
| Auth | PKCE. Redirect must be `http://127.0.0.1:PORT` (no `localhost`) | PKCE (S256). Client credentials only reach the catalog | Classic OAuth (`app_id` + `secret`). **New app registration closed** |
| Live calls without credentials | Useless: known routes return 401, unknown 410, removed ones still 401 | Useless: real prefixes return 401, unknown 404 | **Useful**: public reads work. Auth-only routes return error code 200, unknown paths 600 |
| Limits | Search max 10, add 100 per request, cover is raw base64 JPEG ≤ 256 KB | Add 50 per request, cursor pagination, cover INTERNAL-only, no `PRIVATE` on create | HTTP 200 + `{"error":…}` for all errors, quota 50 req / 5 s (code 4), `/playlist/{id}` embeds ≤ 400 tracks, `artist:"…"` search filter broken |

progenitor 0.15.1 (reqwest 0.13) has these limits:
- OAS 3.0.x only.
- Unique operationIds required.
- Request bodies must be JSON, octet-stream, form or text.
- It **panics on more than one success or more than one error response type per operation**. That hits all 411 live TIDAL operations
  (8 different `DefaultNNNResponseBody` error schemas), plus Spotify `GET /me/player` and `/me/library*`.
- It ignores `discriminator`. typify turns TIDAL's `Included` (88 `$ref`s, untagged) into an enum tried in spec order,
  so **every included track would silently decode as `Downloads_Resource_Object`**.

## Layout

```
Cargo.toml              [workspace] members = ["music-api"]; converter package (edition 2024) depends on music-api
src/main.rs             converter CLI + unit tests
music-api/
  build.rs              spec JSON → normalize (in memory) → progenitor → $OUT_DIR/{spotify,tidal}.rs
  specs/fetch.sh        re-pins both specs (curl; Spotify YAML → JSON with `npx -y js-yaml`)
  specs/spotify.json    vendored, byte-identical to upstream
  specs/tidal.json      vendored, byte-identical to upstream
  src/lib.rs            pub mod auth; pub mod spotify; pub mod tidal; pub mod deezer;
  src/auth.rs           PKCE, loopback redirect, token exchange
  src/spotify.rs        include! generated + client() + upload_playlist_cover()
  src/tidal.rs          include! generated + client() + pre-hook restoring vnd.api+json headers
  src/deezer.rs         hand-written client: models + every endpoint
  tests/common/mod.rs   Prism::start(spec): spawn, wait for the port, kill on Drop
  tests/{spotify,tidal,deezer}.rs
```

## Design

### Spotify + TIDAL: generated at build time
- `build.rs` runs, for each spec: `serde_json::Value` → normalize → `openapiv3::OpenAPI` →
  `progenitor::Generator` (**Positional** interface) → `prettyplease` → `$OUT_DIR/<svc>.rs`, with `rerun-if-changed` on the specs.
  Positional instead of Builder because Builder also emits a builder per type, which more than doubles the generated code.
- Normalization happens in memory only, so the vendored files stay upstream-exact and Prism tests against the real docs:
  - Both specs:
    - Drop `deprecated: true` operations.
    - Keep only the first 2xx response and drop the non-2xx ones, which avoids the multi-response panic.
      Errors then arrive as `Error::UnexpectedResponse(reqwest::Response)`, which still carries the status and `Retry-After`.
  - Spotify: drop `upload-custom-playlist-cover` (`image/jpeg` body) and hand-write it instead.
  - TIDAL:
    - Set `operationId = {method}_{path}` snake-cased.
    - Rename media type `application/vnd.api+json` → `application/json`. A `pre_hook_async` puts back `Content-Type`/`Accept: application/vnd.api+json` on the wire.
    - Set `Included.items = {}` so `included` stays raw `serde_json::Value`.
    - Replace the 70 to-one relationships written as `oneOf[{$ref}, {nullable, enum:[null]}]` with a plain `$ref`, so they become `Option`.
  - Anything else the first build trips on gets patched or dropped, each listed in `build.rs`. That drop list is the only gap in "full coverage".
- `spotify.rs` / `tidal.rs`:
  - `BASE_URL` and `client(base, token) -> Client`: a reqwest client with a default `Authorization: Bearer` header.
  - Spotify also gets `upload_playlist_cover(&Client, id, jpeg)`: `PUT /playlists/{id}/images`, standard base64 **with padding**,
    `Content-Type: image/jpeg`. The old prototype used `URL_SAFE_NO_PAD`, which is the wrong alphabet.
  - TIDAL:
    - The hook is `async fn vnd_api(r: &mut reqwest::Request) -> Result<(), Infallible>`, given by absolute path.
    - `included_tracks(&[Value]) -> Vec<types::TracksResourceObject>` keeps `type == "tracks"` and decodes each with `from_value`.
    - The current user is `/users/{id}` with `id = "me"` (there is no `/users/me` path).
- Dependencies:
  - Normal: `progenitor-client 0.15`, `reqwest 0.13` (json, query, stream, multipart), `serde`, `serde_json`, `futures`, plus whatever the generated code needs (chrono/uuid/regress).
  - Build: `progenitor 0.15` (no default features), `openapiv3`, `serde_json`, and `syn` + `prettyplease` at the versions progenitor's own `example-build` uses.

### Deezer: hand-written `deezer.rs`
- `Deezer { http, base, token: Option<String> }` with private `get/post/delete<T>`.
  - The token travels as the `access_token` param.
  - The body is parsed as a `Value` first. `{"error":…}` becomes `Error::Api { code, kind, message }`, and code 4 (quota) sleeps 5 s and retries.
- `Page<T> { data, total, next }` plus an `all()` helper that pages by `index`/`limit` and never follows `next` URLs.
  Playlist tracks always come from `/playlist/{id}/tracks`, because the embedded list stops at 400.
- Models are written from real responses (Album, Artist, Track with `isrc`, Playlist, User, Genre, Radio, Podcast, Episode, Chart, Editorial, Infos, Options…).
  Fields that aren't always present are `Option`.
- Endpoint inventory = the official reference (Appendix). Where the docs have gaps, use yvasyliev/dz4j and browniebroke/deezer-python:
  - Parameters that aren't documented: batch "add album(s)/artist(s)/playlist(s)/track(s)", playlist update fields.
  - The undocumented but live endpoints: `/track/isrc:{isrc}`, `/album/upc:{upc}`, and cover upload at `upload.deezer.com/playlist/{id}`
    (multipart `file` + `upload_token` from `/infos`).
  - Every path is verified against production with the credential-less probe (see Testing).
- `add-with-options` (playlist tracks, user albums/artists/playlists/tracks: "add with custom add dates, preserving history, e.g. for DSP imports"):
  - Its doc pages return 404, but the live API confirms the routes exist for POST: the 600 error reads "…for request_method get".
  - Discover its params during implementation from the 501 error messages of token-less POSTs. Wrap it if they turn out usable;
    otherwise list it as the one known gap.
- The raw export stays out of git: it's Deezer's content and has your account name in the page header.
- OAuth:
  - Login at `connect.deezer.com/oauth/auth.php?app_id&redirect_uri&perms=basic_access,manage_library,offline_access`.
  - Exchange at `access_token.php?app_id&secret&code&output=json`. `offline_access` gives a token that doesn't expire.

### Auth `auth.rs`
- `Pkce::new()`: 64-char verifier, S256 challenge (`sha2` + `base64` url-safe no pad, `rand`).
- `wait_for_code(state)`: one-shot std `TcpListener` on `127.0.0.1:8888`. It parses `GET /callback?code&state`, checks `state` and answers "close this tab".
  Deezer's login uses the same listener.
- `exchange(token_url, form) -> Token { access_token, refresh_token, expires_in }`.
- Endpoints: Spotify `accounts.spotify.com/authorize` and `/api/token`; TIDAL `login.tidal.com/authorize` and `auth.tidal.com/v1/oauth2/token`.
- Scopes: Spotify `playlist-read-private playlist-read-collaborative playlist-modify-public playlist-modify-private ugc-image-upload`;
  TIDAL `user.read playlists.read playlists.write search.read`.

### Converter `src/main.rs`
- Usage: `playlist-converter <playlist-url> --to spotify|deezer|tidal`, parsed with `std::env::args`.
  Credentials come from env: `SPOTIFY_CLIENT_ID`, `TIDAL_CLIENT_ID`, `DEEZER_APP_ID`, `DEEZER_SECRET`.
- Accepted URLs: `open.spotify.com/playlist/{id}`, `spotify:playlist:{id}`, `deezer.com/[lang/]playlist/{id}`, `[listen.]tidal.com/[browse/]playlist/{uuid}`.
- `Track { isrc, title, artist }` and `Playlist { name, description, cover_url, tracks }`. Each service gets `read`, `find(&Track)` and `write(&Playlist, ids)`.
  Log in only when needed: public Deezer playlists need no login.
- Matching:
  - ISRC first: Spotify `search q=isrc:X type=track`, TIDAL `GET /tracks?filter[isrc]=X`, Deezer `/track/isrc:X`.
  - Otherwise a plain text search for "artist title", taking the first result whose artist matches case-insensitively (`ponytail:` naive match).
- Writes:
  - Create the playlist, then add tracks in batches (Spotify 100, TIDAL 50, Deezer 100). TIDAL playlists are created `UNLISTED`.
  - Copy the cover to Spotify (skip if over 256 KB once encoded) and Deezer. TIDAL can't set covers, so print a note.
- Resilience:
  - One `retry` helper for 429 that honours `Retry-After`.
  - Pagination loops stop when the cursor repeats.
- Output: the new playlist URL, matched X/Y, and the list of unmatched tracks.
- `Cargo.toml` loses `rspotify`, `deezer-rs`, `env_logger`, `base64` and reqwest `blocking`, and runs `git rm deezer-rs rspotify`.

## Testing without credentials

| Target | How | Catches |
|---|---|---|
| Spotify and TIDAL clients | **Prism** (`npx -y @stoplight/prism-cli@5.16.0 mock`) serves the vendored, un-normalized spec locally. Tests point the generated client at it with a dummy bearer token and **assert a 2xx** | Prism fails any request that breaks the documented spec: path, query, headers, body schema, Content-Type (415), Accept (406), missing Bearer token. It answers with data generated from the schema, so response decoding is checked too |
| TIDAL `included` decoding | One hand-written `playlists/{id}/relationships/items?include=items` document, decoded through `included_tracks` | The untagged-enum trap: Prism's static mode always puts an AcceptedTerms object in `included[0]`, so it can't catch this |
| Deezer reads | Live calls to `api.deezer.com` public endpoints with known IDs (your playlist 11241913244, ISRC USUM71703861…) | Models vs real data |
| Deezer writes / auth-only routes | Live calls through the client with **no token**. Each must return OAuthException code 200, never 600 (unknown path). They're rejected before any effect | Wrong paths in our client, error parsing |
| `auth.rs` | RFC 7636 Appendix B PKCE vector and the callback-line parser | Unit tests |
| Converter | URL parsing, match fallback order, batch splitting | Unit tests in `main.rs` |

- `Prism::start` runs npx in its own process group (`CommandExt::process_group(0)`). On Drop it sends `kill -TERM -<pgid>`.
  A plain `Child::kill()` sends SIGKILL to npx only, which orphans Prism and leaves the port taken.
- One `#[tokio::test]` per service runs its calls one after another against a single Prism instance.
  - Spotify: me, playlist, playlist items, search by ISRC and by text, track, create playlist, add items, cover upload, album, artist.
  - TIDAL: `users/{id=me}`, playlists `filter[owners.id]=me`, playlist, items `include=items`, tracks `filter[isrc]`, `searchResults?filter[query]&include=tracks`, create, add items, update, album, artist.
- Coverage of the other generated operations comes from the build itself: anything the generator can't handle fails `cargo build` or lands in the explicit drop list.
- Blind spot: production responses that break their own spec (e.g. `null` in a required field). These show up on the first real run and are fixed with a normalizer patch.

## Tasks, models and skills

**How it runs:**
- I (the main session, Opus 5.5) orchestrate on a branch `revive-music-api` and commit once per task.
- Each task goes to a `general-purpose` subagent with the model and effort below. Approving this plan authorizes those effort settings.
- Parallel tasks run with `isolation: "worktree"`, and I merge them.
- Each brief is self-contained: this plan's section, the files the task may touch, its "done when" check, the skills to load,
  and the comment rules from your `~/.claude/CLAUDE.md`.
- T1 adds every dependency and module stub up front. Parallel tasks then touch disjoint files and never edit `Cargo.toml`/`Cargo.lock`;
  a task that needs a new crate reports back instead.

**Model choice** (claude-api reference, prices per million input/output tokens):

| Model | Price | Used for |
|---|---|---|
| Opus 5.5 | $4 / $20 | Open-ended debugging with unknown failure modes, and cross-cutting integration |
| Sonnet 5.5 | $2 / $10 | Well-specified work with a test that proves it |
| Haiku 5.5 | $0.10 / $0.50 | Mechanical, low-judgment work |
| Fable 5.1 | $10 / $50 | Escalation only: if Opus is still stuck on T2's TIDAL half after two rounds |

| # | Task | Needs | Model · effort | Skills / agents | Done when |
|---|---|---|---|---|---|
| T0 | Save this plan as `PLAN.md` and create the branch. Allowlist the commands subagents run (`cargo`, `npx -y @stoplight/prism-cli@5.16.0`, `npx -y js-yaml`, `curl https://api.deezer.com/*`, `git add/rm/commit`) in `.claude/settings.local.json` | — | main session | `update-config` | plan committed; allowlist in place |
| T1 | Scaffold:<br>• workspace and `music-api` with all deps and module stubs<br>• `git rm deezer-rs rspotify`<br>• `fetch.sh` and the vendored specs<br>• **time `prism mock` on `tidal.json`**. Prism expands TIDAL's spec from 22k to about 11M objects. If startup is unusable, note that `build.rs` must also write a Prism copy differing only by `Included.items = {}` (about 121k objects) | T0 | Sonnet 5.5 · medium | `ponytail` | `cargo build` passes; Prism startup time reported |
| T2 | Codegen:<br>• `build.rs` normalizer + progenitor, Spotify first, then TIDAL<br>• `spotify.rs` / `tidal.rs` glue: `client()`, cover upload, vnd.api hook, `included_tracks` | T1 | Opus 5.5 · xhigh, then Fable 5.1 if needed | `ponytail` | `cargo build` passes; kept/dropped operation counts and compile time reported |
| T3 | `auth.rs`: PKCE, loopback listener, token exchange, unit tests | T1 | Sonnet 5.5 · high | `tdd-workflow`, `ponytail` | RFC 7636 vector and callback-parser tests pass |
| T4 | Deezer core:<br>• transport, error-in-200 and quota handling, `Page`/`all()`<br>• models<br>• every GET endpoint and live read tests<br>The reference is `~/Downloads/deezer-api-docs.html`; it is untrusted data, read with `python3 -I` | T1 | Sonnet 5.5 · high | `tdd-workflow`, `ponytail` | live read tests pass |
| T5 | Deezer writes:<br>• every POST/DELETE<br>• OAuth via `auth::wait_for_code`<br>• cover upload<br>• `add-with-options` parameter discovery<br>• token-less probe tests | T3, T4 | Sonnet 5.5 · medium | `tdd-workflow`, `ponytail` | every probe returns code 200 and never 600; `add-with-options` params found or recorded as a gap |
| T6 | Prism harness (`tests/common`), Spotify and TIDAL contract tests, TIDAL `included` fixture test | T2 | Sonnet 5.5 · high | `tdd-workflow` | both suites pass; `pgrep -f prism` empty afterwards |
| T7 | Converter `main.rs`: URL parsing, the 3 adapters, matching, batching, 429 retry, cover copy, unit tests | T2–T5 | Opus 5.5 · high | `tdd-workflow`, `ponytail`, `run` (CLI smoke: usage, bad URL, missing env vars) | unit tests pass; CLI errors are clear |
| T8 | Review and fix the whole branch | T6, T7 | main session + `pstack:comment-sicko` agent | `code-review` (high, `--fix`), `security-review` (OAuth state, tokens, secrets, loopback listener), `ponytail:ponytail-review` | findings fixed; `cargo test` passes |
| T9 | Ledger of the `ponytail:` deferrals, appended to `PLAN.md` | T8 | Haiku 5.5 · low | `ponytail:ponytail-debt` | ledger committed |
| T10 | End-to-end run with your accounts (browser logins, env vars) | T8 | main session, with you | `run` | the manual conversions under Verification succeed |

**Order:** T0 → T1 → {T2, T3, T4} in parallel → {T5, T6} in parallel → T7 → T8 → T9 → T10.
That's 8 subagents (T1–T7, T9) plus the comment reviewer, inside the 10-agent guideline. I'll name it if a task's model needs changing mid-way.

## Verification
- `cargo build`: every kept operation of both pinned specs generates and compiles.
- `cargo test`: everything above. Needs `npx` and network, no credentials.
- Manual, with your accounts:
  - `SPOTIFY_CLIENT_ID=… cargo run -- https://www.deezer.com/playlist/11241913244 --to spotify`
  - then `--to tidal`
  - then a Spotify playlist `--to deezer`

## Known limits
- Spotify dev mode: the app owner needs Premium (since 2026-03-09), and only playlists you own or collaborate on are readable.
- Deezer:
  - New apps can't be registered. `--to deezer` and private Deezer playlists need an app created before the closure, and its redirect domain must accept `127.0.0.1`.
  - Public Deezer playlists need nothing.
- TIDAL:
  - Transfers break their guidelines without approval (risk accepted).
  - Covers can't be set.
  - Some catalog endpoints may want a client-credentials token, which needs the client secret.
- The generated TIDAL module is large, so the first compile is slow.
- Deferred:
  - Token caching between runs (you log in on every run).
  - Token refresh: Spotify tokens last 1 h.
  - Fuzzy matching.
  - Keeping the source's track add-dates when converting into Deezer (needs `add-with-options`).

## Appendix: Deezer inventory (official reference, exported 2026-10-20)

- **Objects** (`GET /{object}/{id}`): album, artist, chart, editorial, episode, genre, infos, options, playlist, podcast, radio, track, user.
  Also `search` and `oembed?url=`.
- **GET connections:**
  - album: fans, tracks
  - artist: top, albums, fans, related, radio, playlists
  - chart: tracks, albums, artists, playlists, podcasts
  - editorial: selection (`date`), charts, releases
  - genre: artists, podcasts, radios
  - playlist: fans, tracks, radio
  - podcast: episodes
  - radio: genres, top, tracks, lists
  - search: base, album, artist, history, playlist, podcast, radio, track, user.
    Params `q`, `strict=on`, `order` (RANKING, TRACK_ASC … DURATION_DESC), advanced `artist:`/`album:`/`track:`/`label:`/`dur_min`/`dur_max`/`bpm_min`/`bpm_max`
  - user: albums, artists, charts (+ tracks, albums, playlists, artists), flow, followings, followers, history, notifications,
    permissions, options, personal_songs, playlists, podcasts, radios, recommendations (+ albums, releases, artists, playlists, tracks, radios), tracks
- **POST:**
  - `episode/{id}/bookmark` (`offset`)
  - `playlist/{id}` (update)
  - `playlist/{id}/seen`
  - `playlist/{id}/tracks` (`songs` to add, or `order` to reorder)
  - `track/{id}` (personal track)
  - `user/{id}/albums`, `artists`, `followings` (`user_id`), `notifications` (`message`), `playlists` (`title` creates; without it, adds a favourite),
    `podcasts` (`podcast_id`), `radios` (`radio_id`), `tracks`
- **DELETE:**
  - `episode/{id}/bookmark`
  - `playlist/{id}`
  - `playlist/{id}/tracks` (`songs`)
  - `track/{id}`
  - `user/{id}/albums` (`album_id`), `artists` (`artist_id`), `followings` (`user_id`), `playlists` (`playlist_id`), `podcasts` (`podcast_id`),
    `radios` (`radio_id`), `tracks` (`track_id`)
- **POST `add-with-options`** on `playlist/{id}/tracks` and `user/{id}/{albums,artists,playlists,tracks}`: undocumented params (see above).
- **Errors** (HTTP 200, `{"error":{"type","message","code"}}`):
  - 4 QUOTA
  - 100 ITEMS_LIMIT_EXCEEDED
  - 200 PERMISSION
  - 300 TOKEN_INVALID
  - 500 PARAMETER
  - 501 PARAMETER_MISSING
  - 600 QUERY_INVALID
  - 700 SERVICE_BUSY
  - 800 DATA_NOT_FOUND
  - 901 INDIVIDUAL_ACCOUNT_NOT_ALLOWED
- **Permissions:** basic_access, email, offline_access, manage_library, manage_community, delete_library, listening_history.
- **Global params:**
  - `index`, `limit`, `access_token`.
  - `output` (JSON only for us).
  - `request_method` (lets a GET act as a POST or DELETE; unused).
  - Quota is 50 requests per 5 s.
- **OAuth:**
  - Server-side flow: `auth.php` (`app_id`, `redirect_uri` on the app's domain, `perms`) → `access_token.php` (`app_id`, `secret`, `code`, `output=json`).
  - A client-side flow (`response_type=token`, no secret) also exists, but it returns the token in the URL fragment, so the server-side flow is used.
