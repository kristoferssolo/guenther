# Guenther

Guenther is a Rust Telegram bot that takes social media links and sends back the media instead of making people click through the usual nonsense.

It currently supports:

- Instagram posts, reels, and TV posts, including photos and galleries
- TikTok short links
- X/Twitter posts
- YouTube Shorts

## Features

- Accepts supported URLs in chat and replies with downloaded media
- Handles multiple supported links in one message in their order of appearance
- Reads links from message text and document or media captions, while ignoring duplicates
- Serves repeat links instantly from Telegram's servers using cached `file_id`s
- Records delivered links per chat and shows who shares what with `/stats`
- Re-encodes videos and photos over Telegram's upload limit, and links to the source when they still do not fit
- Uses random caption lines from `comments.txt`
- Can optionally replace those captions with LLM-written ones that react to the media and the sender's message
- Can optionally answer in character when someone replies to one of its messages
- Extracts post text from image-only X/Twitter posts when available
- Uses a private Cobalt instance for media from every supported platform
- Reads Cobalt account cookies from `cobalt-cookies.json`, which Instagram now requires for reel video
- Can answer inline queries with saved voice lines
- Can optionally capture incoming voice and audio messages into `voice_lines.toml`
- Can show the next F1 weekend, qualifying, sprint, and race times
- Can run concurrent season and race-weekend F1 bingo games with interactive 5×5 cards

## Requirements

Guenther expects these services and tools at runtime:

- [Cobalt](https://github.com/imputnet/cobalt) for social media downloads
- `ffmpeg` and `ffprobe` for shrinking oversized media, voice-line capture, and LLM caption frames
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
- `DATABASE_URL`: SQLx SQLite URL; defaults to `sqlite://data/bingo.sqlite3`; also stores the media `file_id` cache and link stats
- `VOICE_LINES_PATH`: override the path to `voice_lines.toml`
- `FFMPEG_BIN`: override the `ffmpeg` executable
- `FFPROBE_BIN`: override the `ffprobe` executable
- `TELEGRAM_API_URL`: Bot API server to use instead of `https://api.telegram.org`, such as a proxy or a [local Bot API server](#local-bot-api-server); upload limits stay at the cloud values unless `TELEGRAM_API_LOCAL` is set
- `TELEGRAM_API_LOCAL`: `true` when `TELEGRAM_API_URL` runs `telegram-bot-api --local`; raises the upload limit to 2000 MB and reads files from the server's data directory; ignored without `TELEGRAM_API_URL`
- `TELEGRAM_API_DIR`: the local server's data directory (its `--dir`), mounted at the same path; defaults to `/var/lib/telegram-bot-api`; only used with `TELEGRAM_API_LOCAL`

LLM captions and replies (`llm-captions` feature only):

- `OPENROUTER_API_KEY`: OpenRouter API key; LLM captions and replies stay off unless this and `OPENROUTER_MODEL` are set
- `OPENROUTER_MODEL`: OpenRouter model id with image input, for example `deepseek/deepseek-v4.1-flash` or `z-ai/glm-5.3-flash`
- `OPENROUTER_CAPTION_WAIT_SECS`: how long media waits for the model's caption before it is sent with a static one; defaults to `8`, and `0` always sends immediately
- `OPENROUTER_REPLY_COOLDOWN_SECS`: minimum time between two in-character replies in one chat; defaults to `30`, and `0` disables the limit
- `OPENROUTER_VIDEO_INPUT`: `true` sends videos up to 20 MiB to the model instead of extracted frames; needs a video-capable model such as `z-ai/glm-5.3-flash`

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

To enable LLM-written captions (also set the `OPENROUTER_*` variables):

```bash
cargo run --features llm-captions
```

Media waits up to `OPENROUTER_CAPTION_WAIT_SECS` for the model's caption. If
the caption is late, the media is sent with a `comments.txt` caption that is
edited into the model's caption once it answers; on any failure the static
caption stays. The model gets the platform, the sender's first name and message, X post
text, and up to four images: photos as-is and three ffmpeg frames per video.
Repeat links fetch their cached media back from Telegram for the same purpose.
Requests ask OpenRouter to avoid providers that store or train on data.

The same setup answers text replies to Guenther's own messages in character,
as a reply to the user's message. The model gets Guenther's message text or
caption, its photo or video fetched back from Telegram, and the replier's first
name and text. Telegram only exposes the message that was replied to, so
longer threads have no further context. Each chat gets at most one answer per
`OPENROUTER_REPLY_COOLDOWN_SECS`; replies during the cooldown, replies from
other bots, and model failures get no answer. A reply that also contains a
supported link is handled as a media link instead.

### Large media

The cloud Bot API accepts uploads up to 50 MB, and photos up to 10 MB. Before
uploading, Guenther checks each downloaded file against these limits:

- Oversized videos up to 10 minutes long are re-encoded with ffmpeg at a
  bitrate computed from their ffprobe duration. Longer videos are not
  transcoded. Only one transcode runs at a time.
- Oversized photos are downscaled to at most 2560 px per side.
- Items that still do not fit are left out. The chat gets a short notice with
  the source link instead, and the rest of an album is still sent. Partial
  deliveries are not cached, so repeat links send the notice again.

A self-hosted [`telegram-bot-api`](https://github.com/tdlib/telegram-bot-api)
server in `--local` mode raises the file limit to 2000 MB. Set
`TELEGRAM_API_URL` to its address and `TELEGRAM_API_LOCAL=true`; see
[Local Bot API server](#local-bot-api-server). In local mode `getFile` returns
absolute paths on the server's filesystem instead of serving downloads, so
Guenther reads those files directly. It needs read access to the server's
data directory at the same path; Compose mounts it read-only. Guenther refuses
local paths outside `TELEGRAM_API_DIR`, and all local paths when
`TELEGRAM_API_LOCAL` is off.

To enable F1 bingo:

```bash
cargo run --features bingo
```

The SQLite database is always opened at startup and applies its schema through
embedded migrations. Besides F1 bingo, it stores the media `file_id` cache that
lets repeat links be served from Telegram's servers without re-downloading, and
the `media_posts` log of delivered links behind `/stats`. The `bingo` Cargo
feature only gates the bingo commands, card rendering, and callback-query
handler.

## Docker

The repository includes a multi-stage `Dockerfile` and a `compose.yml`.

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

Build with LLM captions as well:

```bash
RUST_FEATURES="--features=bingo,llm-captions" docker compose up --build
```

The Compose setup starts a private Cobalt sidecar that is reachable only from
Guenther's Docker network. It does not expose Cobalt on a host port.

Cobalt reads account cookies from `cobalt-cookies.json`, mounted at
`/cookies.json`. Copy the template and fill in the services you need:

```bash
cp cobalt-cookies.example.json cobalt-cookies.json
```

The file must exist before `docker compose up`, or Docker creates a directory
in its place. Cobalt rewrites refreshed cookies in place, so it is mounted
read-write and its contents will change over time.

Instagram no longer serves video URLs to anonymous clients. Without a logged-in
`sessionid` (or an `instagram_bearer` token), Cobalt falls back to the post's
cover image, and reels arrive as a single JPEG instead of a video.

If YouTube rejects the VPS IP with `error.api.youtube.login`, configure an
HTTP(S) proxy with a different network exit in `.env` and recreate Cobalt:

```env
COBALT_PROXY_URL=http://username:password@proxy.example:8080
```

```bash
docker compose up -d --force-recreate cobalt bot
```

The proxy is used only for Cobalt's outbound requests. It does not receive the
Telegram bot token, and no YouTube account is required.

The bot reads `.env` and mounts:

- `comments.txt`
- `voice_lines.toml`
- a named `bingo-data` volume at `/app/data`
- the local Bot API server's `telegram-bot-api-data` volume, read-only, at
  `/var/lib/telegram-bot-api`

The runtime image installs `ffmpeg` and `ffprobe` for shrinking oversized
media, voice-line capture, and LLM caption frames.

### Local Bot API server

`compose.yml` includes an optional `telegram-bot-api` service in `--local`
mode behind the `local-bot-api` profile. The server writes files readable only
by their owner, so it runs as Guenther's user (uid 1001) and a one-off
`telegram-bot-api-init` service hands its data volume to that user. Without
the profile neither service starts, and Guenther uses the cloud Bot API.

To move the bot to the local server:

1. Stop Guenther, so it stops polling the cloud Bot API:

   ```bash
   docker compose stop bot
   ```

2. Log the bot out of the cloud Bot API. Otherwise both servers may take its
   updates.

   ```bash
   curl "https://api.telegram.org/bot$TELOXIDE_TOKEN/logOut"
   ```

3. Get an `api_id` and `api_hash` from <https://my.telegram.org> and add them
   to `.env` with the server's address:

   ```env
   TELEGRAM_API_ID=123456
   TELEGRAM_API_HASH=0123456789abcdef0123456789abcdef
   TELEGRAM_API_URL=http://telegram-bot-api:8081
   TELEGRAM_API_LOCAL=true
   ```

   Set `TELEGRAM_API_DIR` as well only if you change the server's `--dir`.

4. Start the local server and Guenther with the profile:

   ```bash
   docker compose --profile local-bot-api up -d --build
   ```

To move back to the cloud Bot API:

1. Stop Guenther, leaving the local server running:

   ```bash
   docker compose stop bot
   ```

2. Log the bot out of the local server, so it stops taking the bot's updates:

   ```bash
   docker compose --profile local-bot-api exec telegram-bot-api \
     wget -qO- "http://127.0.0.1:8081/bot$TELOXIDE_TOKEN/logOut"
   ```

3. Remove `TELEGRAM_API_URL`, `TELEGRAM_API_LOCAL`, and `TELEGRAM_API_DIR`
   from `.env`. Guenther otherwise keeps calling the stopped server and fails
   to start.
4. Stop the local server and start Guenther without the profile:

   ```bash
   docker compose --profile local-bot-api stop telegram-bot-api
   docker compose up -d
   ```

The cloud Bot API refuses the bot for 10 minutes after its `logOut`, so wait
that long before moving back right after a move to the local server.

### Build caching in Dokploy

The Dockerfile caches Cargo downloads, compiled dependencies, and incremental
build state with BuildKit cache mounts. Source changes can reuse dependencies
as long as subsequent deployments use the same Docker builder and retain its
cache. The first build after cache deletion compiles all dependencies again.

Dokploy's **Daily Docker Cleanup** runs `docker builder prune --all --force`,
which deletes these caches. Disable it in the instance's Docker settings to
retain build caches between days. This setting affects all applications on the
instance. Also leave Compose cache cleanup disabled.

Check disk and cache usage with `docker system df`. When reclaiming build cache,
prefer an age limit, for example `docker builder prune --filter 'until=168h'`,
to preserve cache used within the last week. Avoid unfiltered builder or system
pruning when investigating slow rebuilds.

## Commands

The bot currently exposes:

- `/help` or `/h` or `/?`: show command help
- `/curse`: send a random Guenther-style line
- `/weekend` or `/f1`: show the next F1 weekend schedule, including practice sessions when available
- `/quali`: show the next F1 qualifying sessions, including sprint qualifying when available
- `/race`: show the next F1 race sessions, including the sprint when available
- `/countdown`: show how long until the next F1 session of the upcoming race weekend starts
- `/standings`: show the current F1 driver and constructor standings
- `/stats [week|month|year|all]`: show this chat's delivered links per sender and per platform, plus links shared more than once; defaults to `all`
- `/bingo`: show F1 bingo help when built with the `bingo` feature

## F1 Bingo

Bingo games are isolated per Telegram chat. A chat can have several active
games at once – for example, a full-season game alongside a race-weekend game-but
one game is the default for commands that omit a game slug. Each card has 24
entries and a pre-marked F1-themed center cell. `LIGHTS OUT!` is the default
center text and chat administrators can customize it per game. Games can also
have an introductory description, which is shown on every card.

Anyone in the chat can list games and entries, add entries, or retrieve cards:

```text
/bingo games
/bingo entries [game]
/bingo add <entry>
/bingo add <game> | <entry>
/bingo generate [game]
/bingo get
/bingo get all
/bingo get [game] @username
```

`/bingo get all` retrieves every card in the chat's default active game.

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

Bingo queries use SQLx's compile-time checked macros. Docker and CI builds use
`SQLX_OFFLINE=true` with the checked-in `.sqlx` metadata, without starting a
database or installing `sqlx-cli`. After changing queries, regenerate the
metadata with `cargo sqlx prepare` against a migrated SQLite database configured
by `DATABASE_URL` and commit the updated `.sqlx` files.

## Inline Voice Lines

Inline queries search entries from `voice_lines.toml` and return cached Telegram voice messages. When built with the `voice-line-capture` feature, the bot can also:

- store incoming voice messages for reuse
- convert incoming audio files to Telegram voice format with `ffmpeg`
- append newly captured items to `voice_lines.toml`

## Notes

- All social media downloads go through Cobalt and support public media only.
  Instagram additionally requires account cookies in `cobalt-cookies.json`; see
  [Docker](#docker).
- Cached entries record what was downloaded at the time. If Cobalt returned
  cover images for reels, purge them so the next request re-downloads:
  `DELETE FROM media_cache WHERE url LIKE 'instagram:%'`.
- Media sent for a previously seen link is reused from Telegram's servers via
  cached `file_id`s; captions are still generated per request. If a cached send
  fails, the entry is invalidated and the media is downloaded again.
- Every delivered link is recorded in `media_posts` with its chat, source
  message, sender id and name, platform, cache key, URL, time, and whether it
  was a cache hit. Failed links are not recorded. To forget a chat's history:
  `DELETE FROM media_posts WHERE chat_id = <chat id>`.
- X/Twitter post text and the image-only fallback use its public syndication endpoint.
- Do not point `COBALT_API_URL` at `api.cobalt.tools`; hosted Cobalt instances are not intended for third-party projects without permission.
- Guenther is intended for self-hosting.
