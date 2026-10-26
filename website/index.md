---
layout: home
title: SoCLI, a thread-first terminal client
titleTemplate: false
description: Cross-platform TUI social media client for X, Bluesky, dev/null, Threads and Mastodon.
---

<div class="stage">

<div class="stage-copy">

<SoMark />

<h1>Cross-platform TUI social media client for X, Bluesky, dev/null, Threads and Mastodon.</h1>

<InstallCommand />

<p class="meta"><span>0.1.2</span> <a href="https://github.com/massimodeluisa/socli/blob/main/LICENSE">MIT</a> <span><code>npx soclish --demo</code> needs no account</span></p>

<nav class="nets" aria-label="Networks">
  <a href="docs/networks.html#x">X</a>
  <a class="sky" href="docs/networks.html#bluesky">Bluesky</a>
  <a class="amber" href="docs/networks.html#dev-null">dev/null</a>
  <a class="violet" href="docs/networks.html#threads">Threads</a>
  <a class="indigo" href="docs/networks.html#mastodon">Mastodon</a>
  <a class="dim" href="https://github.com/massimodeluisa/socli/issues/new?title=Request%20a%20network&body=Network%3A%0A%0AWhat%20should%20SoCLI%20read%20and%20write%20there%3F%0A%0AAPI%20documentation%3A%0A">Request one</a>
</nav>

</div>

<div class="stage-demo">

<DemoReader />

</div>

</div>

<div class="features" id="features">

<section>

<h2><a href="docs/reader.html">Unified social feed</a></h2>

Read your networks in one full-screen TUI, with Home, Discover and Saved feeds, mentions and search. Move through posts with the keyboard or mouse, and filter the feed to the networks you want to read.

</section>

<section>

<h2><a href="docs/commands.html">Direct CLI commands</a></h2>

Read, search and act from your shell. Run commands such as `socli search "rust" --net bluesky --json`, or use `--print --no-prompt` for terminal text. The command reference covers arguments, options and examples.

</section>

<section>

<h2><a href="docs/commands.html#write">Post across networks</a></h2>

Publish text to several networks with `socli post "hello" --net x,bluesky`. Reply, quote, like, repost and bookmark where the network supports it. The TUI composer keeps drafts, and `--dry-run` checks a command before you send it.

</section>

<section>

<h2><a href="docs/reader.html#split-view">Threads and profiles</a></h2>

Follow conversations on thread rails, expand folded replies and open an author's profile. On wide terminals, split view keeps the feed beside the selected conversation so you can read both at once.

</section>

<section>

<h2><a href="docs/reader.html#detail-levels">Terminal appearance and images</a></h2>

Choose themes, spacing and four detail levels, from plain text to inline pictures and link previews. View images in the terminal with kitty, iTerm2 or sixel graphics, with half blocks as the fallback.

</section>

<section>

<h2><a href="docs/agents.html">JSON for scripts and agents</a></h2>

Pipe feeds and command results to other tools with `--json`. Coding agents get JSON output automatically, without interactive prompts. Per-network write results and exit codes let callers check what succeeded.

</section>

<section id="web">

<h2><a href="docs/web.html">WebAssembly in the browser</a></h2>

The reader on this page is the SoCLI program, compiled to WebAssembly, with the same keys and layout. It opens the welcome screen until this site has an account. Bluesky, Mastodon, Threads and X sign in and read here. X and Threads open an approval page when this build includes SoCLI's app id. A build without that id says so. X on the page uses the official API, so For you is a search, and likes and follows can be refused. The local docs server forwards only `api.x.com` for the X token exchange, and a static host such as GitHub Pages does not. Threads sends the approval code to `https://socli.sh/threads/token`, which holds the app secret. The Threads app is published. dev/null signs in on the page: it opens the approval on dev-null.sh, and you paste the `devnull://` callback. GitHub SSH signing stays in the terminal. `socli login x` in a terminal still imports the browser session. The page keeps its config, tokens included, in this origin's local storage. Rich mode draws profile pictures and post images on the page, including a signed-in timeline, by loading each image from its URL. `ui.images` set to `off` keeps the image card. kitty, iTerm2 and sixel stay in the terminal. The buttons above the reader set a mobile, tablet, or desktop window, and full screen opens a page that is only the reader.

</section>

</div>

<section class="keys" id="keys" aria-labelledby="keys-title">

<h2 id="keys-title">Keys</h2>

<!--@include: ./docs/keys.md#reader-keys-->

See [all keyboard shortcuts](docs/keys.html) for composer, dialog and Settings controls.

</section>
