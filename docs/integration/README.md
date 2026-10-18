# Network integrations

socli reads and writes five networks through one provider layer: X, Bluesky, dev/null, Threads and Mastodon. Each network sits behind the `Provider` trait in `src/net/mod.rs`, which turns its API into the shared `Post`, `User` and `Page` types, so the screens, the merged timeline and the commands never see a network-specific response. This page compares what the five networks can do, explains how ids, credentials and colors work across them, and describes what a new network has to provide. Reddit and Baidu Tieba are not integrated and have their own pages: [Reddit](reddit/README.md) and [Tieba](tieba/README.md).

Every network has a folder with five pages: `README.md` (capabilities, limits, status, sources), `auth.md`, `endpoints.md`, `mapping.md` and `testing.md`.

| Network | Slug | Pages |
| --- | --- | --- |
| X | `x` | [x/](x/README.md) |
| Bluesky | `bluesky` | [bluesky/](bluesky/README.md) |
| dev/null | `devnull` | [devnull/](devnull/README.md) |
| Threads | `threads` | [threads/](threads/README.md) |
| Mastodon | `mastodon` | [mastodon/](mastodon/README.md) |
| Reddit | not integrated | [reddit/](reddit/README.md) |
| Baidu Tieba | not integrated | [tieba/](tieba/README.md) |

## Capabilities

The matrix shows what each provider reports in its `Caps` (`src/net/mod.rs`) once signed in. A cell reads "no" when the provider returns an "unsupported" error for that action, for example "Threads has no likes in its API".

| | X | Bluesky | dev/null | Threads | Mastodon |
| --- | --- | --- | --- | --- | --- |
| Feeds | For you, Following, Bookmarks | Following, Discover, Bookmarks | Following, Latest, Trending, Starred | Your threads | Home, Trending, Bookmarks |
| Feed merged into All | Following | Following | Following | Your threads | Home |
| Mentions | yes | yes | yes | yes | yes |
| Search | yes | yes | no | yes | yes |
| Thread | yes | yes | yes | yes | yes |
| Profile | yes | yes | yes | own account only | yes |
| Post and reply | yes | yes | yes | yes | yes |
| Quote | yes | yes | yes | yes | yes |
| Like or star | like | like | star | no | favorite |
| Repost | yes, with undo | yes, with undo | no | yes, no undo | yes, with undo |
| Bookmark | yes | yes | no | no | yes |
| Follow | yes | yes | yes | no | yes |
| Delete | yes | yes | yes | yes | yes |
| Counts | yes | yes | yes | no | yes |
| Views | yes | no | no | no | no |
| Length limit | 280, weighted | 300 graphemes | 4000 characters | 500, emoji by UTF-8 bytes | the server's limit, 500 by default, graphemes |

Some cells change with the sign-in state. These are the states the code distinguishes:

- X with a browser session (`socli login x`, or `X_AUTH_TOKEN` and `X_CT0`): the full row, including For you and Following from the website and a native quote.
- X with only an app bearer token (`X_BEARER_TOKEN` or `socli auth x bearer`): thread, profile and counts of public posts, plus search and views. No feeds, no posting.
- Bluesky signed out: thread, profile and counts of public posts, limit 300. No feeds and no writing.
- Mastodon signed out: thread and profile of public posts, reading from `mastodon.social` unless a server is named. No feeds, no search and no writing.
- dev/null public: the same public set, limit 4000. With an SSH key (`socli auth devnull ssh --key`) it adds posting. With an approved device (`socli login devnull`) it gains the full set in the table.

A reply needs the same capability as a post. An X quote on a browser session is native. An X quote on the official API, a Threads quote, and a Mastodon quote of another network's post are sent as your text followed by the link. A quote of a post on the same network is native where the API has one (see each network's mapping page). On Mastodon that holds for servers running 4.5 or newer, and an older server gets the link appended too.

A feed name that a network lacks falls back where the code defines it: `latest` and `discover` open For you on X, and `discover` opens Trending on dev/null and on Mastodon. X has no trending feed in socli.

Every network reports a rate limit with the same wording: "<network> rate limit reached; <wait>", with the network written as `X`, `Bluesky`, `Threads`, `dev/null` or `Mastodon` (for example "X rate limit reached; try again in 15 minutes"). The wait reads "try again in 45s" below 120 seconds, "try again in 15 minutes" from 120 seconds up (rounded up), and "try again in a minute" when the response gives no wait. X takes the wait from `x-rate-limit-reset` and falls back to `retry-after`, Threads takes it from `Retry-After`, and Mastodon takes it from `x-ratelimit-reset` (an ISO 8601 time) and falls back to `Retry-After`.

## Ids and links

Every post id is scoped as `<slug>:<native id>`, for example `x:<id>`, `threads:<id>`, `devnull:<id>`, `mastodon:<status id>` or `bluesky:at://<did>/app.bsky.feed.post/<rkey>`. `net::parse_target` turns the argument of commands such as `thread`, `reply`, `quote`, `like` and `delete` into a scoped id:

- A scoped id is used as written.
- A link on `x.com` or `twitter.com` (with `www.` or `mobile.` removed) of the form `/<user>/status/<digits>` becomes `x:<digits>`.
- A link `bsky.app/profile/<actor>/post/<rkey>` becomes `bluesky:at://<actor>/app.bsky.feed.post/<rkey>`.
- A `dev-null.sh` link whose path contains `commits/<id>` becomes `devnull:<id>`.
- A Mastodon link of the form `https://<server>/@<user>/<digits>`, `https://<server>/@<user>@<other>/<digits>` or `https://<server>/users/<user>/statuses/<digits>` becomes `mastodon:<link>`. The link stays the id because a status id is valid only on the server that issued it, and socli resolves it through search when the server is not yours (see [mastodon/mapping.md](mastodon/mapping.md)).
- A link on `threads.net` or `threads.com` is refused: "Threads post links cannot be resolved through the API; use the id from `socli --json`".
- Any other link fails with "socli cannot read <input>. use a post id, or a link from x.com, bsky.app, dev-null.sh or a Mastodon server".
- A bare id uses the network from a single `--net`, the selected scope, or `default_network`. When none applies, socli asks "which network? add --net x|bluesky|devnull|threads|mastodon, or use a full id like bluesky:at://…".

`--net` takes one slug, a comma list or `all`. It accepts the aliases `twitter`, `bsky`, `dev/null`, `dev-null` and `masto`. The `SOCLI_NET` environment variable sets the default, and `default_network` in the configuration does the same when it is unset.

## Credentials and configuration

socli reads `config.toml` from `$SOCLI_CONFIG_DIR`, else `$XDG_CONFIG_HOME/socli`, else `~/.config/socli`. The file is written with mode 0600 in a 0700 directory, and writes are atomic. Each network has one section. The X and Threads variables override the stored values, and the Bluesky variables are inputs to `socli login bluesky`, and the Mastodon variables work both ways: they are inputs to `socli login mastodon` and they override the stored values at run time.

| Network | Section keys | Environment variables | Sign in |
| --- | --- | --- | --- |
| X | `auth_token`, `ct0`, `client_id`, `client_secret`, `access_token`, `refresh_token`, `expires_at`, `user_id`, `username`, `bearer_token`, `latest_query` | `X_AUTH_TOKEN`, `X_CT0`, `X_CLIENT_ID`, `X_CLIENT_SECRET`, `X_ACCESS_TOKEN`, `X_REFRESH_TOKEN`, `X_USER_ID`, `X_USERNAME`, `X_BEARER_TOKEN` | `socli login x` (browser session). `--oauth` uses a developer app |
| Bluesky | `service`, `handle`, `did`, `access_jwt`, `refresh_jwt` | `BLUESKY_HANDLE`, `BLUESKY_APP_PASSWORD`, `BLUESKY_SERVICE` | `socli login bluesky` (`--handle`, `--service`, `--password-stdin`) |
| dev/null | `username`, `device_key`, `device_approved`, `ssh_key` | none for credentials | `socli login devnull` (`--device-name`) |
| Threads | `user_id`, `username`, `access_token`, `issued_at`, `expires_at` | `THREADS_ACCESS_TOKEN`, `THREADS_APP_SECRET` | `socli login threads` |
| Mastodon | `server`, `client_id`, `client_secret`, `access_token`, `user_id`, `username`, `max_chars` | `MASTODON_ACCESS_TOKEN`, `MASTODON_SERVER` | `socli login mastodon` (`--server`, `--token`); without `--server` or `MASTODON_SERVER` it asks for the server, and an empty answer keeps `mastodon.social` |

Other variables that affect the integrations: `SOCLI_NET`, `SOCLI_CONFIG_DIR`, `SOCLI_CACHE_DIR`, `SOCLI_AGENT`, `SOCLI_THEME`, `SOCLI_DENSITY`, `SOCLI_DETAIL`, `SOCLI_LAYOUT`, `SOCLI_MOUSE`, `SOCLI_ICONS`, `SOCLI_IMAGE`, `SOCLI_HYPERLINKS` and `NO_COLOR`. The `[<network>]` sections also hold the `color` key described below. `socli auth status` shows what is stored without printing secrets, and each network's `auth.md` has the details.

## The merged timeline

The All view combines the networks that are on (`feed.networks`, default `all`). Its three tabs ask each network for one feed kind and skip a network that has none:

| Tab | X | Bluesky | dev/null | Threads | Mastodon |
| --- | --- | --- | --- | --- | --- |
| Home (the primary feed) | Following | Following | Following | Your threads | Home |
| Discover | For you | Discover | Trending | skipped | Trending |
| Saved | Bookmarks | Bookmarks | Starred | skipped | Bookmarks |

X contributes For you to Discover through the alias above, and dev/null and Mastodon contribute Trending. In Saved, dev/null contributes the commits you starred, because it has no bookmarks. The chips row above the list shows the networks and the tabs, for example `[All]  ● X  ● Bluesky  ● dev/null  ● Threads  ● Mastodon  │  [1 Home]  2 Discover  3 Saved`. A signed-out network contributes nothing, because it has no feeds. `feed.replies` sets the order of replies in a thread (default `oldest`).

## Network colors

Each network has a color for its dot and its handle. The key `[<network>] color` sets it, and the defaults are `default` for X, `sky` for Bluesky, `amber` for dev/null and `violet` for Threads and `indigo` for Mastodon (`socli config set x.color teal`, for instance). A value is a palette name (`default`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet`, `indigo`) or a hex color such as `#ff8800`. The key `feed.color_by` takes `auto`, `network` or `author` (default `auto`): `auto` uses network colors when the feed mixes networks, `network` colors every post in its network's color, and `author` gives every author a color of their own.

## Adding a network

A network is a module under `src/net/<slug>/` that implements `Provider`, plus an entry in the `Network` enum, the `Clients` registry and the configuration. The pieces in `src/net/mod.rs`:

- `Provider` has these methods: `network`, `caps`, `signed_in`, `account`, `me`, `feed`, `mentions`, `thread`, `user`, `search`, `post`, `delete`, `set_like`, `set_repost`, `set_bookmark` and `set_follow`. A method for something the network lacks returns `NetError::unsupported`, which prints as "<network> has no <thing> in its API", for example "Threads has no likes in its API".
- `Caps` declares what the interface may offer: `feeds` (tabs with labels), `primary`, `mentions`, `search`, `like`, `repost`, `unrepost`, `quote`, `bookmark`, `follow`, `delete`, `thread`, `user`, `post`, `counts`, `views`, `like_verb`, `max_chars` and `count_graphemes`. `Caps::public(max_chars)` is the starting point for a client that can only read public threads and profiles.
- `Clients` is the registry. `Clients::load` builds the clients from the configuration, and the other methods are `empty`, `get`, `providers_mut`, `infos` and `signed_in`. Bluesky, dev/null and Mastodon always exist so public reads work signed out, X and Threads exist when credentials are present.
- The length rule of the network lives in `Network::text_len`, and `Network::profile_url` builds profile links.

A complete integration also needs the five pages in `docs/integration/<slug>/` (`README.md`, `auth.md`, `endpoints.md`, `mapping.md`, `testing.md`) and a row in the tables on this page.
