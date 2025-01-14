use deezer_rs::{Deezer};
use rspotify::{prelude::*, scopes, AuthCodeSpotify, Credentials, OAuth};
use base64::{engine::general_purpose, Engine as _};
use reqwest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // set logger
    env_logger::init();

    // The credentials must be available in the environment. Enable the
    // `env-file` feature in order to read them from an `.env` file.
    let creds = Credentials::from_env().unwrap();

    // set scopes
    let scopes = scopes!(
        "user-read-email",
        "user-read-private",
        "user-top-read",
        "user-library-read",
        "playlist-read-collaborative",
        "playlist-read-private",
        "ugc-image-upload",
        "playlist-modify-public",
        "playlist-modify-private"
    );

    let oauth = OAuth::from_env(scopes).unwrap();

    let spotify = AuthCodeSpotify::new(creds, oauth);

    let url = spotify.get_authorize_url(false).unwrap();
    // This function requires the `cli` feature enabled.
    spotify.prompt_for_token(&url).await.unwrap();

    // get current user id
    let user = spotify.current_user().await.unwrap();

    // get deezer playlist
    let deezer_client = Deezer::new();
    let playlist = deezer_client.playlist().get("11241913244").await.unwrap();

    let created_playlist = spotify
        .user_playlist_create(
            user.id,
            playlist.title.as_str(),
            Some(true),
            Some(false),
            Some(playlist.description.unwrap().as_str()),
        )
        .await
        .unwrap();
    let img_url = "https://images.dog.ceo/breeds/poodle-toy/n02113624_8936.jpg";
    let img_bytes = tokio::task::spawn_blocking(move || {reqwest::blocking::get(img_url).unwrap().bytes().unwrap()}).await?;

    let playlist_cover_base64 = general_purpose::URL_SAFE_NO_PAD.encode(img_bytes.clone());
    let decode = general_purpose::URL_SAFE_NO_PAD.decode(playlist_cover_base64.as_str()).unwrap();
    // let playlist_cover_base64 = general_purpose::STANDARD_NO_PAD.encode(playlist.picture);
    assert!(img_bytes == decode);

    let token = spotify.token.lock().await.unwrap();
    println!("Access token: {}", &token.as_ref().unwrap().access_token);
    println!("byte64 : {}", playlist_cover_base64);
    println!("playlist id : {}", created_playlist.id);

    spotify
    .playlist_upload_cover_image(
        created_playlist.id.as_ref(),
         playlist_cover_base64.as_str()
        )
         .await
         .unwrap();

    let res = spotify.playlist_cover_image(created_playlist.id.as_ref()).await.unwrap();
    println!("{:?}", res.height.unwrap());

    // fill up tracks model
    // for track in playlist.tracks {
    //     spotify.seek_track(position, device_id)
    // }



    Ok(())
}
