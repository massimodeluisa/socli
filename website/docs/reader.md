---
title: Reader
description: What the full-screen reader shows, the keys it takes, the detail levels it draws and the themes it ships.
---

# Reader

`socli` opens the reader when both input and output are terminals and you pass neither `--json` nor `--print`. With more than one network signed in, the reader shows a merged timeline. Without an account, it opens a sign-in screen with an option to try the offline demo. `socli --print` writes the threads into the scrollback and, at a terminal, leaves a `socli>` prompt. Add `--no-prompt` to print once and exit. The [CLI reference](commands.html) covers output modes and the prompt's command syntax.

The reader has three sections: Home, Notifications and Search, and `[` and `]` step through them. Home, Discover and Saved are the tabs of the merged timeline. Home takes each network's primary feed, Discover takes For you on X, Discover on Bluesky and Trending on dev/null and Mastodon, and Saved takes bookmarks, plus the commits you starred on dev/null.

When the selection reaches the 10th post of the current page in a feed, a search, a profile, or mentions, the reader draws a loading card and fetches the next page. A shorter page starts on its last post. Each signed-in network is asked for at least 20 posts. Medium and rich put `loading…` in a box. Minimal and plain write the words only, and plain spells them `loading...`. That fetch runs while the keys still move. Pressing `m` loads one page the same way. `ctrl-r` and `cmd-r` reload the page on screen, and `r` still replies. An open thread has nothing further to fetch, and `--demo` stays on the posts it already has.

A post is a letter badge on a rail. Replies hang under the post, and a long thread folds until you press `⏎`. In a feed that mixes networks the badge takes the network's color, and on a single network it takes a color from the author's handle, so the same account keeps the same color. The `feed.color_by` setting chooses what the badge follows.

## On the website

The reader on the [homepage](../index.html) is this program compiled to WebAssembly. With no account stored for the site, it opens the same welcome screen as a terminal. Bluesky, Mastodon, Threads and X sign in and read from the page. X and Threads open an approval page when this build includes socli's app id. A build without that id says so. X on the page uses the official API, so For you is a search, and likes and follows can be refused. The local docs server forwards only `api.x.com` for the X token exchange, and a static host such as GitHub Pages does not. Threads sends the approval code to `https://socli.sh/threads/token`, which holds the app secret. The Threads app is published. dev/null signs in on the page as well: it opens the approval on dev-null.sh, and you paste the `devnull://` callback. GitHub SSH signing stays in the terminal (`socli auth devnull ssh`). `socli login x` in a terminal still imports the browser session. The config, tokens included, is kept in this origin's local storage under `socli.config`. A terminal keeps its own file. Rich mode draws profile pictures and post images on the page, including a signed-in timeline, by loading each image from its URL. `ui.images` set to `off` keeps the image card. Drafts are not kept. kitty, iTerm2 and sixel stay in the terminal. A page fetch waits inside the reader worker, then the screen updates. The stage stays one column until the window can hold 124 columns, which is when the terminal splits the feed and the thread. The buttons above the reader can set a mobile, tablet, or desktop window instead, and full screen opens `/web/`. The welcome screen can still open the offline demo. If the WebAssembly file does not load, the page shows a smaller scripted feed instead.

## Keys {#keys}

The [Keys reference](keys.html) lists reader shortcuts and the controls for the composer, pickers, share dialog, image viewer and Settings. Press `?` in the reader to open its key overlay.

## Detail levels

The detail level sets how much art the reader and `--print` draw. Pick it with `--detail`, `SOCLI_DETAIL` or `ui.detail`. With none of those set, a normal terminal opens at rich. An ESP terminal, or a `TERM` of `dumb`, `unknown`, `ansi`, `vt52`, `vt100`, `vt102`, `vt220` or `network`, opens at plain.

| Level | What it draws |
| --- | --- |
| `plain` | No color, ASCII chrome, and `>` marks the selected post |
| `minimal` | No boxes or icons in posts, and a colored side line per network |
| `medium` | Rails, letter badges and a one-line media row, drawn to the edges |
| `rich` | Medium plus inline images, link preview images, profile pictures and turning spinners |

In the reader, `d` steps to the next level and `D` opens a picker. Either choice is written to `ui.detail` and used the next time you open socli. `--detail` and `SOCLI_DETAIL` still win over that saved value. A narrow terminal changes the layout and never the level.

Link preview images come from the network's card data or from the page's `og:image`. Turn the page fetch off with the "Fetch link pages" setting or `ui.unfurl`. Rich previews appear in the full-screen reader and not in `--print`.

## Split view

On a terminal at least 124 columns wide, the reader shows the feed on the left and the selected thread on the right. The right pane loads that conversation's replies, and the other posts stay on the left. `--layout split` lowers that width to 100 columns, `--layout single` turns it off, and the setting is `ui.layout`. `⏎` or `→` moves the keys into the thread, `⌫`, `←` or `esc` moves them back to the feed, and the side without the keys is dimmed. Below that width, `⏎` opens the thread on the one pane.

## Profiles

`u` opens the profile of the selected post's author, and `:user @handle` opens the profile of any account. The frame holds the name, the handle, the bare profile URL and the biography, and a `Posts` section lists the loaded posts. `o` opens the profile page in a browser, and `s` shares the profile link.

## Composer

`n` opens the composer. In the composer `⏎` sends, `ctrl-j` adds a newline and `esc` goes back. `⇧⏎` also adds a newline in terminals that report the kitty keyboard protocol. `socli config set ui.enter newline`, or the `Enter in composer` row in Settings, swaps the two, so `⏎` adds a newline and `ctrl-s` sends.

The text field moves and deletes whole characters, accents and emoji included. A paste goes in as text with its newlines and never sends. [Composer and editing keys](keys.html#composer) covers word movement, undo, destinations and saved drafts.

Leaving a composer that holds text asks whether to save a draft. A send writes the draft before it reaches any network and removes it once every network has taken the post, so after a failed send the draft keeps the text and the networks that failed. `socli drafts` lists the saved drafts and marks one `may have posted` when socli stopped before it knew the outcome, and `socli drafts rm <n>` removes one.

## Mouse

The reader takes the mouse. A click on a post selects it and a second click opens it, and a click on a name, a time, a link, a mention, a hashtag, a stats cell, a network chip, a feed tab, a nav section or a hint presses it, the same as moving the focus ring there and pressing `⏎`. The wheel moves the selection, or the cursor of an open picker or dialog, and hovering over a picker row moves the cursor to it.

While socli has the mouse, the terminal selects text only with Shift-drag, which is Option-drag in iTerm2. `--mouse off`, `SOCLI_MOUSE=off`, `socli config set ui.mouse off` or the Mouse row in Settings leaves the mouse to the terminal.

## Themes

socli ships `auto` (black or white, following the terminal background), `native` (the terminal's own colors and a transparent background), `black`, `white` and 27 named palettes adapted from opencode, such as `nord`, `gruvbox` and `catppuccin`. A named theme follows the terminal: it uses its light variant on a light terminal when it has one, and `nord-dark` or `nord-light` forces a variant.

`socli config themes` lists every name. Choose one with `--theme`, `SOCLI_THEME`, `ui.theme`, the Theme row in Settings, or `:theme`, which opens a picker that previews each theme as the cursor moves.
