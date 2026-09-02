# Guenther

Guenther is a Rust Telegram bot that takes social media links and sends back the media instead of making people click through the usual nonsense.

It currently supports:

- Instagram posts, reels, and TV posts, including photos, videos, and galleries
- TikTok short links
- X/Twitter posts
- YouTube Shorts

## Features

- Accepts supported URLs in chat and replies with downloaded media
- Serves repeat links instantly from Telegram's servers using cached `file_id`s
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
- a SQLite database file (created automatically at `data/bingo.sqlite3` by default)

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
- `DATABASE_URL`: SQLx SQLite URL; defaults to `sqlite://data/bingo.sqlite3`; also stores the media `file_id` cache
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable when using voice-line capture

Sample `.env`:

```env
TELOXIDE_TOKEN=123456:telegram-token
CHAT_ID=123456789
COBALT_API_URL=http://127.0.0.1:9000/
ENABLED_PLATFORMS=instagram,tiktok,twitter,youtube
F1_UTC_OFFSET=+3
DATABASE_URL=sqlite://data/bingo.sqlite3
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

The SQLite database is always opened at startup and applies its schema through
embedded migrations. Besides F1 bingo, it stores the media `file_id` cache that
lets repeat links be served from Telegram's servers without re-downloading. The
`bingo` Cargo feature only gates the bingo commands, card rendering, and
callback-query handler.

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
- `/countdown`: show how long until the next F1 session of the upcoming race weekend starts
- `/standings`: show the current F1 driver and constructor standings
- `/bingo`: show F1 bingo help when built with the `bingo` feature

## F1 Bingo

Bingo games are isolated per Telegram chat. A chat can have several active
games at once – for example, a full-season game alongside a race-weekend game-but
one game is the default for commands that omit a game slug. Each card has 24
entries and a pre-marked F1-themed center cell. `LIGHTS OUT!` is the default
center text and chat administrators can customize it per game. Games can also
have an introductory description, which is shown on every card.

Anyone in the chat can list games and entries, add entries, or retrieve a card:

```text
/bingo games
/bingo entries [game]
/bingo add <entry>
/bingo add <game> | <entry>
/bingo generate [game]
/bingo get
/bingo get [game] @username
```

Only Telegram chat administrators can manage games, bulk-import entries, edit
or delete entries, and manage other users' cards:

```text
/bingo game create <slug> <name>
/bingo game delete <slug>
/bingo game activate <slug>
/bingo game close <slug>
/bingo game default <slug>
/bingo game center <slug> <text>
/bingo game description <slug> [text]

/bingo entries import <game>
/bingo edit [game] <entry_number> <text>
/bingo delete [game] <entry_number>

/bingo generate [game] @username
/bingo regenerate [game] @username
/bingo reset [game] @username
/bingo card set <game> [@username] <A1-E5> <entry_number>
```

Omitting the text from `game description` clears the current description.
Deleting a game permanently removes all of its entries and cards.

Games start in `draft`. Add at least 24 entries and activate the game before
generating randomized cards. Generation samples without replacement and stores
the resulting order. Editing or deleting an entry affects future cards only;
existing cards retain their original text. `regenerate` explicitly replaces an
existing card.

Anyone can generate their own card. Chat administrators can generate,
regenerate, mark, and reset cards for other users.

### Importing an entry list

Chat administrators can upload a UTF-8 text file containing one entry per
line. Blank lines are ignored. Attach the file as a Telegram document and use
this as its caption:

```text
/bingo entries import season-2026
```

The file may contain up to 1,000 non-empty lines and be up to 64 KiB. Duplicate
entries are normalized and merged. The entire import is transactional, so an
invalid entry rejects the file without partially importing it.

Telegram cannot reliably resolve an arbitrary username that the bot has never
seen. If `@username` is unavailable or has changed, reply to one of that user's
messages and omit the username from `generate`, `import`, `reset`, or
`card set`.

### Interactive marking

The bot sends each card as a portrait PNG followed by a complete text version
and a 5×5 inline button grid. The image uses a white background and black grid,
with pale red circles behind marked entries and a pale gold circle behind the
automatic center cell. Long image text is reduced to a readable minimum size
and then ellipsized when necessary; the companion text always retains the full
stored entry for accessibility and copying. Rendering uses an embedded FOSS
Noto Sans Mono bitmap font and does not require system fonts in Docker.

The button coordinates match the card entries. Only the assigned owner can
mark or unmark cells. A button press updates both the existing photo and its
companion text and keyboard. Older text-only buttons remain usable but cannot
update a photo because they do not contain its message ID. Completing any row,
column, or diagonal is a bingo; the center cell participates in those lines.
The bot announces the first completed line once. Closing a game disables
further marking while leaving its cards available through `/bingo get`.

Generate a deterministic local preview at
`target/bingo-card-preview.png` with:

```bash
just bingo-card-preview
```

### Importing existing cards

Use `import` to assemble a card from existing game-local entry numbers without shuffling them.
The five rows map to A through E and each row's five values map to columns 1
through 5. Every number must identify an active entry in the selected game. The
center at C3 must be `*`. Prefix an already-marked number with `[x]`.

The safest way to select the owner is to reply to one of their messages and
omit `@username`:

```text
/bingo reimport 2026-season
[x] 81 | 60 | 38 | 41 | 39
66 | 55 | 67 | 77 | 43
35 | 3 | * | 17 | 1
24 | 61 | 13 | 28 | 19
63 | 30 | 37 | 29 | 33
```

If the bot already knows the owner, use `/bingo import 2026-season @driver` as
the first line instead, followed by the same five grid rows.

Use `/bingo import ...` when the user does not have a card yet, or
`/bingo reimport ...` to explicitly replace an existing card. Individual
mistakes can be corrected later with `/bingo card set`.
Use the game-local entry number shown by `/bingo entries <game>`; the selected entry
must be active and belong to that game.
Set the game's center text before importing if the paper cards use a different
F1-themed center phrase; all cards in one game share that center text.

The database schema is applied automatically at startup through embedded SQLx
migrations. For local runs, back up `data/bingo.sqlite3`; Compose deployments
should back up the `bingo-data` volume.

Bingo queries use SQLx's compile-time checked macros. Builds validate them
online against the migrated SQLite database configured by `DATABASE_URL`.
The container build creates a temporary database for this purpose; `.sqlx` is
ignored because offline mode is not used.

## Inline Voice Lines

Inline queries search entries from `voice_lines.toml` and return cached Telegram voice messages. When built with the `voice-line-capture` feature, the bot can also:

- store incoming voice messages for reuse
- convert incoming audio files to Telegram voice format with `ffmpeg`
- append newly captured items to `voice_lines.toml`

## Notes

- All social media downloads use Cobalt without account cookies and support public media only.
- Media sent for a previously seen link is reused from Telegram's servers via
  cached `file_id`s; captions are still generated per request. If a cached send
  fails, the entry is invalidated and the media is downloaded again.
- X/Twitter post text and the image-only fallback use its public syndication endpoint.
- Do not point `COBALT_API_URL` at `api.cobalt.tools`; hosted Cobalt instances are not intended for third-party projects without permission.
- Guenther is intended for self-hosting.
