#!/bin/sh
set -e
cd "$(dirname "$0")"
curl -fsSL -o spotify.yml https://raw.githubusercontent.com/sonallux/spotify-web-api/main/fixed-spotify-open-api.yml
npx -y js-yaml spotify.yml > spotify.json
rm spotify.yml
curl -fsSL -o tidal.json https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json
