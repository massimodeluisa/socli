---
title: Web implementation
description: The socli reader in the browser, compiled to WebAssembly, with sign-in, live timelines, and the same Rust rendering core as the native CLI.
---

# Web implementation

socli compiles to WebAssembly and runs its terminal reader in the browser. The [homepage](../index.html) uses the same Rust state, layout, themes and keys as the native application.

## On the page

Click the reader to focus it. With no account stored for the site, the welcome screen offers each network and the offline demo. `j` and `k` move, `⏎` opens a conversation and `esc` goes back. Press `?` for the keyboard reference. Touch controls provide movement, open and back actions without a keyboard.

Bluesky, Mastodon, Threads and X sign in and read from the page. X and Threads open an approval page when this build includes socli's app id. A build without that id says so. X on the page uses the official API, so For you is a search, and likes and follows can be refused. The local docs server forwards only `api.x.com` for the X token exchange, and a static host such as GitHub Pages does not. Threads sends the approval code to `https://socli.sh/threads/token`, which holds the app secret. The Threads app is published. dev/null signs in on the page as well: it opens the approval on dev-null.sh, and you paste the `devnull://` callback. GitHub SSH signing stays in the terminal (`socli auth devnull ssh`). `socli login x` in a terminal still imports the browser session. Rich mode draws profile pictures and post images on the page, including a signed-in timeline, by loading each image from its URL. `ui.images` set to `off` keeps the image card. Drafts are not kept. The page draws the same Nerd Font icons as Ghostty. `ui.icons` set to `unicode` keeps the Unicode set. The buttons above the reader set a mobile, tablet, or desktop window. Full screen opens a page that is only the reader, at `/web/`. The host `web.socli.sh` opens that page.

A setting you change while signed in is written to this site's local storage, under the key `socli.config`. That record can hold tokens. A terminal keeps its own file at `~/.config/socli`. The offline demo still does not save, and it still does not send likes, replies, reposts or bookmarks.

The [Keys reference](keys.html) covers the reader's controls. To open the offline feed in a terminal, run `socli --demo`.

## How it works

Rust draws the reader as a grid of styled terminal cells. A module worker runs the WebAssembly. The page paints the cells the worker returns and passes keyboard, paste, pointer and resize events back. The worker performs each network request synchronously, so one session stays in order, and the page paints again when the request returns. A plain-text version of the screen supports assistive technology.

The browser and native application share the reader's behavior. Browser input and rendering replace the terminal-specific parts, while the website keeps its own surrounding layout and colors.

## What stays in the terminal

The X browser-session cookie import, GitHub SSH signing, drafts, file paths, and `socli view`. dev/null device approval happens on the page, by pasting the `devnull://` callback. The page signs in to X with socli's public app when this origin forwards `api.x.com`. Rich mode draws live photos on the page from their URLs. `ui.images` set to `off` keeps the image card. kitty, iTerm2 and sixel stay in the terminal. Pictures are not forwarded. If the WebAssembly file does not load, the page shows the scripted offline demo instead.

For the terminal, follow [Install](install.html), [Sign in](login.html) and the [CLI command reference](commands.html).
