# Guenther

Guenther is a Rust Telegram bot that takes social media links and sends back the media instead of making people click through the usual nonsense.

It currently supports:

- Instagram reels and TV posts (borked)
- TikTok short links
- X/Twitter posts
- YouTube Shorts (borked)

## Features

- Accepts supported URLs in chat and replies with downloaded media
- Uses random caption lines from `comments.txt`
- Extracts post text from image-only X/Twitter posts when available
- Supports optional cookie files for platforms that need authenticated access
- Can answer inline queries with saved voice lines
- Can optionally capture incoming voice and audio messages into `voice_lines.toml`

## Requirements

Guenther expects these tools at runtime:

- `yt-dlp`
- `ffmpeg` (when creating/saving voice lines)
- a Telegram bot token exposed as `TELOXIDE_TOKEN`

## Configuration

Guenther reads configuration from environment variables.

Required:

- `TELOXIDE_TOKEN`: Telegram bot token

Optional:

- `CHAT_ID`: admin/debug chat that receives internal error messages
- `IG_SESSION_COOKIE_PATH`: path to Instagram cookies file
- `TIKTOK_SESSION_COOKIE_PATH`: path to TikTok cookies file
- `TWITTER_SESSION_COOKIE_PATH`: path to X/Twitter cookies file
- `YOUTUBE_SESSION_COOKIE_PATH`: path to YouTube cookies file
- `YOUTUBE_POSTPROCESSOR_ARGS`: custom `yt-dlp` postprocessor arguments for YouTube downloads
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable when using voice-line capture

Sample `.env`:

```env
TELOXIDE_TOKEN=123456:telegram-token
CHAT_ID=123456789
IG_SESSION_COOKIE_PATH=./cookies/www.instagram.com_cookies.txt
TIKTOK_SESSION_COOKIE_PATH=./cookies/www.tiktok.com_cookies.txt
TWITTER_SESSION_COOKIE_PATH=./cookies/www.twitter.com_cookies.txt
YOUTUBE_SESSION_COOKIE_PATH=./cookies/www.youtube.com_cookies.txt
```

## Running Locally

Install dependencies first:

```bash
cargo build
```

Then run the Telegram bot:

```bash
cargo run
```

To disable specific platforms, use Cargo features:

```bash
cargo run --features instagram,tiktok
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

The compose setup mounts:

- `.env`
- `comments.txt`
- `voice_lines.toml`
- platform cookie files into `/app/*.txt`

The runtime image bundles `yt-dlp` and installs `ffmpeg`.

## Commands

The bot currently exposes:

- `/help` or `/h` or `/?`: show command help
- `/curse`: send a random Guenther-style line

## Inline Voice Lines

Inline queries search entries from `voice_lines.toml` and return cached Telegram voice messages. When built with the `voice-line-capture` feature, the bot can also:

- store incoming voice messages for reuse
- convert incoming audio files to Telegram voice format with `ffmpeg`
- append newly captured items to `voice_lines.toml`

## Notes

- The downloader relies on `yt-dlp`, so platform breakage can happen whenever sites change their internals.
- Cookie files are optional, but in practice they help a lot for rate limits, age gates, and platform restrictions.
- Guenther is intended for self-hosting.

## License

Licensed under [MIT license](LICENSE).
