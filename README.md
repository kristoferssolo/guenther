# Guenther

Guenther is a Rust Telegram bot that takes social media links and sends back the media instead of making people click through the usual nonsense.

It currently supports:

- Instagram reels and TV posts
- TikTok short links
- X/Twitter posts
- YouTube Shorts

## Features

- Accepts supported URLs in chat and replies with downloaded media
- Uses random caption lines from `comments.txt`
- Extracts post text from image-only X/Twitter posts when available
- Uses a private Cobalt instance for public Instagram and YouTube media
- Supports optional cookie files for TikTok and X/Twitter
- Can answer inline queries with saved voice lines
- Can optionally capture incoming voice and audio messages into `voice_lines.toml`
- Can show the next F1 weekend, qualifying, sprint, and race times

## Requirements

Guenther expects these services and tools at runtime:

- [Cobalt](https://github.com/imputnet/cobalt) for Instagram and YouTube
- `yt-dlp` for TikTok and X/Twitter
- `ffmpeg` (when creating/saving voice lines)
- a Telegram bot token exposed as `TELOXIDE_TOKEN`

## Configuration

Guenther reads configuration from environment variables.

Required:

- `TELOXIDE_TOKEN`: Telegram bot token

Optional:

- `CHAT_ID`: admin/debug chat that receives internal error messages
- `COBALT_API_URL`: Cobalt processing endpoint; defaults to `http://127.0.0.1:9000/`
- `COBALT_API_KEY`: API key for a protected external Cobalt instance; omit for the private Compose service
- `TIKTOK_SESSION_COOKIE_PATH`: path to TikTok cookies file
- `TWITTER_SESSION_COOKIE_PATH`: path to X/Twitter cookies file
- `F1_UTC_OFFSET`: offset for F1 schedule output, for example `+3` or `+03:00`
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable when using voice-line capture

Sample `.env`:

```env
TELOXIDE_TOKEN=123456:telegram-token
CHAT_ID=123456789
COBALT_API_URL=http://127.0.0.1:9000/
TIKTOK_SESSION_COOKIE_PATH=./cookies/www.tiktok.com_cookies.txt
TWITTER_SESSION_COOKIE_PATH=./cookies/www.twitter.com_cookies.txt
F1_UTC_OFFSET=+3
```

## Running Locally

Start a local Cobalt instance, then install dependencies:

```bash
cargo build
```

Then run the Telegram bot:

```bash
cargo run
```

To enable only specific platforms, disable the default feature set first:

```bash
cargo run --no-default-features --features instagram,tiktok
```

To enable automatic voice-line capture:

```bash
cargo run --features voice-line-capture
```

## Docker

The repository includes a multi-stage `Dockerfile` and a `docker-compose.yml`.

Build and start:

```bash
docker compose up --build
```

The Compose setup starts a private Cobalt sidecar that is reachable only from
Guenther's Docker network. It does not expose Cobalt on a host port or provide
Instagram/YouTube account cookies.

The setup also mounts:

- `.env`
- `comments.txt`
- `voice_lines.toml`
- optional TikTok and X/Twitter cookie files into `/app/*.txt`

The runtime image bundles `yt-dlp` and installs `ffmpeg`.

## Commands

The bot currently exposes:

- `/help` or `/h` or `/?`: show command help
- `/curse`: send a random Guenther-style line
- `/weekend` or `/f1`: show the next F1 weekend schedule, including practice sessions when available
- `/quali`: show the next F1 qualifying sessions, including sprint qualifying when available
- `/race`: show the next F1 race sessions, including the sprint when available

## Inline Voice Lines

Inline queries search entries from `voice_lines.toml` and return cached Telegram voice messages. When built with the `voice-line-capture` feature, the bot can also:

- store incoming voice messages for reuse
- convert incoming audio files to Telegram voice format with `ffmpeg`
- append newly captured items to `voice_lines.toml`

## Notes

- Instagram and YouTube downloads use Cobalt without account cookies and support public media only.
- Do not point `COBALT_API_URL` at `api.cobalt.tools`; hosted Cobalt instances are not intended for third-party projects without permission.
- TikTok and X/Twitter still rely on `yt-dlp`, so platform changes can break those integrations.
- TikTok and X/Twitter cookie files are optional, but may help with rate limits and platform restrictions.
- Guenther is intended for self-hosting.

## License

Licensed under [MIT license](LICENSE).
