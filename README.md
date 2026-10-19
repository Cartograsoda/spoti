# spoti

Spotify in your terminal. It has a mouse, a status line that actually tells you what happened, and your top songs of the year whenever you want them, not whenever Spotify's marketing team decides it's December.

It's also light. The Spotify desktop app can sit at around 1 GB of RAM on Windows, just to play songs. spoti plays the same songs in 50-60 MB.

It's a fork of [spotify-player](https://github.com/aome510/spotify-player) by aome510, who did the hard part. This fork adds the things I wanted and fixes the things that annoyed me.

![Top tracks](docs/screenshots/top-tracks.png)

## Wrapped, without the December wait

Press `g t` (or click **Top**) and you get your 100 most played songs of roughly the last year, with the total runtime on top, so you can find out exactly how much of your life went into one album. Hit `tab` (or click **Artists**) for your top 50 artists over the same stretch, ranked, no judgment.

This is Spotify's own ranking of your listening, straight from their API. It's not the full Wrapped show: no minutes listened, no "listening age", no animated slides telling you you're in the top 0.5% of fans. Spotify doesn't give apps that data. If you want the real numbers, request your **Extended streaming history** under Privacy on your Spotify account page. It takes up to 30 days, which is still earlier than December if you ask now.

Want more or fewer than 100? Set `top_tracks_limit` in `app.toml`.

![Top artists](docs/screenshots/top-artists.png)

## What this fork adds

- **A mouse that does things.** A clickable bar of tabs and buttons: Library, Search, Liked, Top, Queue, Lyrics, play/pause, shuffle, volume, devices. Double-click plays or opens, right-click opens actions, scroll moves through lists, scroll over the player changes the volume, click the progress bar to seek. Whatever's under the cursor lights up, so you know what you're about to click.
- **A status line.** Every action reports back: `Play: done`, `Search "hey you": done`, red with a reason when something fails, and a countdown when Spotify is rate limiting you. No more pressing play five times and wondering.
- **A random button.** Plays a random song from the list you're looking at, or from what's playing, or from your liked songs. For when you have 3,000 liked songs and zero decisions left in you.
- **A queue that fills itself.** Pick a song from search, or paste a track link, and Spotify's recommendations for it line up behind it, like in the official app. No more silence after one song.
- **Fewer rate limits.** Play, pause, skip, seek and volume go straight to the built-in player instead of taking a round trip through Spotify's servers. Your library loads from cache on startup when it's fresh, and bursts of refreshes get merged into one.
- **No silent blank pages.** If Spotify sends back a broken playlist or a page fails to load, the status line says so, instead of the app staring into the void forever.

![A playlist](docs/screenshots/playlist.png)

![Search](docs/screenshots/search.png)

![Lyrics](docs/screenshots/lyrics.png)

| It tells you when things worked | Every time |
| --- | --- |
| ![Play: done](docs/screenshots/status-play.png) | ![Search done](docs/screenshots/status-search.png) |

## Getting it running

You need Spotify Premium (streaming doesn't work without it) and [Rust](https://rustup.rs). Five minutes, tops.

**1. Build it**

```sh
git clone https://github.com/Cartograsoda/spoti
cd spoti
cargo install --path spotify_player --locked --features image
```

`--features image` gets you album covers in the terminal. The binary is called `spotify_player`. I copied mine to `spoti` because life is short.

**2. Make your own Spotify client ID (don't skip this)**

A client ID is how Spotify knows which app is asking for your data, and Spotify caps how many requests each one can make. Without your own, the app borrows ncspot's, the same one every other terminal Spotify user borrows, so you wait in line behind strangers. That's the "it sits there doing nothing for 30 seconds" experience. It's not frozen, it's queueing.

- Create an app on the [Spotify developer dashboard](https://developer.spotify.com/dashboard).
- When it asks which APIs you'll use, tick **Web API** only.
- Add `http://127.0.0.1:8989/login` as a redirect URI. Exactly that, character for character.

**3. Tell spoti about it**

Copy the client ID from the dashboard into `~/.config/spotify-player/app.toml` (create the file if it isn't there):

```toml
client_id = "your-client-id"
```

**4. Log in**

```sh
spotify_player authenticate
```

Approve everything in the browser. It asks more than once; that's normal.

**5. Play something**

```sh
spotify_player
```

One catch: Spotify doesn't let new apps read some things, like songs in other people's playlists or an artist's top tracks. Those requests quietly fall back to the shared ID, so they can still be slow sometimes. Your own playlists stay fast.

## Keys worth knowing

| Key | Does |
| --- | --- |
| `space` | play / pause |
| `n` / `p` | next / previous |
| `.` | random song |
| `g t` | top tracks |
| `g T` | top artists |
| `tab` | switch between top tracks and artists |
| `g y` | liked songs |
| `g l` | library |
| `g s` | search |
| `l` | lyrics |
| `z` | queue |
| `D` | switch device |
| `?` | every other command |
| `q` | quit |

All the config options are in [docs/config.md](docs/config.md). Everything else (themes, Spotify Connect, CLI commands, the daemon) works like upstream, so the [original README](https://github.com/aome510/spotify-player#readme) has you covered.

## Known weirdness

- Artist pages sometimes don't load. Spotify blocks newer apps from artist top tracks and related artists, so those go through the shared ID and get rate limited. Give it a minute.
- Built and used on Windows. Upstream runs on Linux and macOS too and nothing here should break that, but I haven't checked.

## Credits

Built on [spotify-player](https://github.com/aome510/spotify-player) by aome510, with [librespot](https://github.com/librespot-org/librespot), [ratatui](https://github.com/ratatui/ratatui) and [rspotify](https://github.com/ramsayleung/rspotify) doing the heavy lifting. MIT licensed, see [LICENSE](LICENSE).
