#[tokio::main]
async fn main() {
    eprintln!("usage: playlist-converter <playlist-url> --to spotify|deezer|tidal");
    std::process::exit(2);
}
