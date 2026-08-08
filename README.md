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
- Can run concurrent season and race-weekend F1 bingo games with interactive 5×5 cards

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
- `BINGO_DATABASE_URL`: SQLx SQLite URL; defaults to `sqlite://data/bingo.sqlite3`
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable when using voice-line capture

Sample `.env`:

```env
TELOXIDE_TOKEN=123456:telegram-token
CHAT_ID=123456789
COBALT_API_URL=http://127.0.0.1:9000/
ENABLED_PLATFORMS=instagram,tiktok,twitter,youtube
F1_UTC_OFFSET=+3
BINGO_DATABASE_URL=sqlite://data/bingo.sqlite3
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

To enable F1 bingo:

```bash
cargo run --features bingo
```

The `bingo` Cargo feature contains the SQLx dependency, migrations, commands,
and callback-query handler. When the feature is disabled, the database is not
opened and `/bingo` is not registered.

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

Build the Compose bot with bingo enabled:

```bash
RUST_FEATURES=--features=bingo docker compose up --build
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
- a named `bingo-data` volume at `/app/data`

The runtime image installs `ffmpeg` for optional voice-line capture.

## Commands

The bot currently exposes:

- `/help` or `/h` or `/?`: show command help
- `/curse`: send a random Guenther-style line
- `/weekend` or `/f1`: show the next F1 weekend schedule, including practice sessions when available
- `/quali`: show the next F1 qualifying sessions, including sprint qualifying when available
- `/race`: show the next F1 race sessions, including the sprint when available
- `/bingo`: show F1 bingo help when built with the `bingo` feature

## F1 Bingo

Bingo games are isolated per Telegram chat. A chat can have several active
games at once – for example, a full-season game alongside a race-weekend game—but
one game is the default for commands that omit a game slug. Each card has 24
entries and a pre-marked F1-themed center cell. `LIGHTS OUT!` is the default
center text and chat administrators can customize it per game.

Anyone in the chat can list games and entries or retrieve a card:

```text
/bingo games
/bingo entries [game]
/bingo get
/bingo get [game] @username
```

Only Telegram chat administrators can manage games, entries, and cards:

```text
/bingo game create <slug> <name>
/bingo game activate <slug>
/bingo game close <slug>
/bingo game default <slug>
/bingo game center <slug> <text>

/bingo add <entry>
/bingo add <game> | <entry>
/bingo edit <entry_id> <text>
/bingo delete <entry_id>

/bingo generate [game] @username
/bingo regenerate [game] @username
/bingo reset [game] @username
/bingo card set <game> [@username] <A1-E5> <text>
```

Games start in `draft`. Add at least 24 entries and activate the game before
generating randomized cards. Generation samples without replacement and stores
the resulting order. Editing or deleting an entry affects future cards only;
existing cards retain their original text. `regenerate` explicitly replaces an
existing card.

Telegram cannot reliably resolve an arbitrary username that the bot has never
seen. If `@username` is unavailable or has changed, reply to one of that user's
messages and omit the username from `generate`, `import`, `reset`, or
`card set`.

### Interactive marking

The message includes a 5×5 inline button grid whose coordinates match the card
entries. Only the assigned owner can mark or unmark cells. Completing any row,
column, or diagonal is a bingo; the center cell participates in those lines.
The bot announces the first completed line once. Closing a game disables
further marking while leaving its cards available through `/bingo get`.

### Importing existing cards

Use `import` to preserve a manually created card. The first line selects the
game and owner; the following five lines contain five `|`-separated cells each.
The center must be `*`. Prefix an already-completed cell with `[x]`:

```text
/bingo import 2026-season @driver
[x] Safety car | Wet race | Team orders | Red flag | Rookie points
Pit stop error | Photo finish | DNS | Fastest lap | Engine failure
Pole sitter wins | Rain delay | * | Surprise podium | Penalty
Double stack | Radio rant | VSC | First-lap crash | Strategy gamble
Undercut works | Yellow flag | Track limits | Late overtake | Debris
```

Use `/bingo reimport ...` with the same format to explicitly replace an
existing imported card. Imported entry texts are also added to that game's
entry pool. Individual mistakes can be corrected later with `/bingo card set`.

The database schema is applied automatically at startup through embedded SQLx
migrations. For local runs, back up `data/bingo.sqlite3`; Compose deployments
should back up the `bingo-data` volume.

Bingo queries use SQLx's compile-time checked macros. Offline query metadata is
stored in `.sqlx`, so builds and CI do not need a live database. After changing
a query or migration, regenerate that metadata with:

```bash
just sqlx-prepare
```

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
