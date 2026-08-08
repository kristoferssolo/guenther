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
- Uses a private Cobalt instance for media from every supported platform
- Downloads public media without social account cookies
- Can answer inline queries with saved voice lines
- Can optionally capture incoming voice and audio messages into `voice_lines.toml`
- Can show the next F1 weekend, qualifying, sprint, and race times

## Requirements

Guenther expects these services and tools at runtime:

- [Cobalt](https://github.com/imputnet/cobalt) for social media downloads
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
- `COBALT_PROXY_URL`: HTTP(S) proxy used only by the Compose Cobalt service; useful when a hosting provider's IP is blocked by a media platform
- `ENABLED_PLATFORMS`: comma-separated platforms to enable; defaults to all platforms
- `F1_UTC_OFFSET`: offset for F1 schedule output, for example `+3` or `+03:00`
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable when using voice-line capture

Sample `.env`:

```env
TELOXIDE_TOKEN=123456:telegram-token
CHAT_ID=123456789
COBALT_API_URL=http://127.0.0.1:9000/
ENABLED_PLATFORMS=instagram,tiktok,twitter,youtube
F1_UTC_OFFSET=+3
```

Supported platform names are `instagram`, `tiktok`, `twitter` (or `x`), and
`youtube`. Use `all` or leave the variable unset to enable everything. Set it
to an empty value to disable all media handlers. Changes take effect when the
bot restarts.

## Running Locally

Start a local Cobalt instance, then install dependencies:

```bash
cargo build
```

Then run the Telegram bot:

```bash
cargo run
```

To enable only specific platforms at runtime:

```bash
ENABLED_PLATFORMS=instagram,tiktok cargo run
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

Platform selection can also be passed directly to Compose:

```bash
ENABLED_PLATFORMS=instagram,youtube docker compose up --build
```

The Compose setup starts a private Cobalt sidecar that is reachable only from
Guenther's Docker network. It does not expose Cobalt on a host port or provide
social account cookies.

If YouTube rejects the VPS IP with `error.api.youtube.login`, configure an
HTTP(S) proxy with a different network exit in `.env` and recreate Cobalt:

```env
COBALT_PROXY_URL=http://username:password@proxy.example:8080
```

```bash
docker compose up -d --force-recreate cobalt bot
```

The proxy is used only for Cobalt's outbound requests. It does not receive the
Telegram bot token, and no YouTube account or cookies are required.

The bot reads `.env` and mounts:

- `comments.txt`
- `voice_lines.toml`

The runtime image installs `ffmpeg` for optional voice-line capture.

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

- All social media downloads use Cobalt without account cookies and support public media only.
- X/Twitter post text and the image-only fallback use its public syndication endpoint.
- Do not point `COBALT_API_URL` at `api.cobalt.tools`; hosted Cobalt instances are not intended for third-party projects without permission.
- Guenther is intended for self-hosting.

## License

Licensed under [MIT license](LICENSE).
