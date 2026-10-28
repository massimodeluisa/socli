---
title: Images
description: How SoCLI draws pictures in the terminal, and how to choose the graphics mode.
---

# Images

Use [Ghostty](https://ghostty.org/docs/install/binary) instead of Terminal.app. Install it from the [official installation page](https://ghostty.org/docs/install/binary). Ghostty speaks the kitty graphics protocol, so profile pictures and post images render. Terminal.app does not. SoCLI paints half blocks there, and it does not send Terminal the kitty probe, because Terminal prints that probe and any kitty or iTerm image as text.

Images need a graphics protocol: kitty, iTerm2 or sixel. Without one, SoCLI paints half blocks, and inside tmux it always uses half blocks. The `auto` mode asks every terminal other than Terminal.app what it supports and takes the best protocol it finds.

## In the reader

`v` opens the viewer for the selected post's images. The rich detail level draws inline pictures in the feed and link previews with the image the page carries, and a profile picture replaces the letter badge when the terminal has a graphics protocol. On the website, rich mode draws those same pictures in the browser, loading each one from its URL. `ui.images` set to `off` keeps the image card.

## From the shell

```sh
socli view demo
socli view x:1848213… --json
```

`socli view` shows a post's images in the terminal. It takes a post id, a post link, an image URL, a file or `demo`, and `--json` prints the image URLs instead of drawing them.

Pictures from posts and link pages load only from public addresses, and SoCLI checks every redirect before it follows one. A local file opens only when you pass its path to `socli view <path>`.

## Choosing the mode

`--images[=auto|kitty|iterm|sixel|blocks|off]` picks the mode, `SOCLI_IMAGE` sets it when the flag is absent, and `ui.images` sets it in the config file. The Settings screen takes the same six values, and `--images` on its own means `auto`. The `off` mode draws no pictures and drops the `v view` hints.

In `sixel` mode the inline images use sixel, while the full-screen viewer and `--print` use half blocks, because the viuer library is built without sixel. Profile pictures replace the letter badges only with the kitty, iTerm2 or sixel graphics.

Cursor and VS Code draw images only when `terminal.integrated.enableImages` is on.
