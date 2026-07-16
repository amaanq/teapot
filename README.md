# teapot

A privacy-focused Twitter/X frontend written in Rust.

## Features

- **Privacy-focused**: No third-party JavaScript, tracking, ads, or remote fonts
- **RSS feeds**: Subscribe to any user's tweets
- **FxEmbed-style Discord embeds**:
  - Multiple images displayed in carousel on mobile
  - Videos play directly in the embed
  - Proper image dimensions for better layout
  - ActivityPub JSON endpoint for rich embeds

## Requirements

- Rust 1.96.1+
- Twitter/X session tokens for API access

## Building

```bash
cargo build --release
```

## Configuration

1. Copy the example config:

```bash
cp config/teapot.example.toml config/teapot.toml
```

1. Edit `config/teapot.toml` with your settings.

2. Create a sessions file with your Twitter/X credentials:

```bash
cp sessions.example.jsonl sessions.jsonl
# Edit sessions.jsonl with your auth_token and ct0 from browser cookies
```

Cookie sessions use `auth_token` and `ct0`. OAuth sessions use
`oauth_token` and `oauth_secret`. See `sessions.example.jsonl` for the JSONL
shape. Never commit a real session or generated `teapot.toml`.

## Running

```bash
# Development
cargo run

# Production
./target/release/teapot
```

The server will start on `http://localhost:8080` by default.

## Getting Twitter Sessions

To use the Twitter API, you need session tokens from a logged-in Twitter account:

1. Log into Twitter/X in your browser
2. Open Developer Tools (F12) → Application → Cookies
3. Copy the values of `auth_token` and `ct0`
4. Add them to `sessions.jsonl`

## Project Structure

```
teapot/
├── src/
│   ├── main.rs          # Entry point
│   ├── config.rs        # Configuration
│   ├── error.rs         # Error types
│   ├── types/           # Data structures (User, Tweet, Timeline, etc.)
│   ├── api/             # Twitter API client, OAuth, parsing
│   ├── cache/           # In-process caching
│   ├── routes/          # HTTP route handlers
│   ├── views/           # Maud HTML templates
│   └── utils/           # Utilities (HMAC, formatters)
├── public/              # Static assets (CSS, JS, fonts)
├── config/              # Configuration files
└── Cargo.toml           # Dependencies
```

## Embed Improvements

This rewrite includes FxEmbed-style improvements for Discord embeds:

### Multiple Images

Discord can display all images from a tweet in a carousel. This works via an ActivityPub JSON endpoint at `/users/{username}/statuses/{id}` that returns media attachments.

### Video Playback

Videos can be played directly in Discord embeds using `twitter:player` meta tags:

- `twitter:player` - Embed URL
- `twitter:player:stream` - Direct MP4 URL
- `twitter:player:width/height` - Dimensions

### Image Dimensions

All images include `og:image:width` and `og:image:height` meta tags for proper layout.

## License

[GNU Affero General Public License v3.0](LICENSE)
