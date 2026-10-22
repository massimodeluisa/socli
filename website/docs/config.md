---
title: Configuration
description: Where socli keeps its settings, every config key, and the environment variables that override them.
---

# Configuration

socli reads `$SOCLI_CONFIG_DIR/config.toml`, else `$XDG_CONFIG_HOME/socli/config.toml`, else `~/.config/socli/config.toml`. The file has mode `0600` in a directory with mode `0700`, and every write replaces it atomically.

## Commands

```sh
socli config list
socli config get ui.theme
socli config set ui.theme nord
socli config path
socli config themes
```

`socli config list` shows every key with its value or default, `socli config get` reads one key, and `socli config set` validates a value and saves one key. `socli config path` prints where the file is, and `socli config themes` lists every name that `ui.theme` accepts.

In the reader, a notice that a setting was saved reads `saved` and leaves out the path.

## Keys

| Key | Values | Default |
| --- | --- | --- |
| `feed.networks` | `all` or a comma list of `x`, `bluesky`, `devnull`, `threads`, `mastodon` | `all` |
| `feed.color_by` | `auto`, `network`, `author` | `auto` |
| `feed.replies` | `oldest`, `newest` | `oldest` |
| `default_network` | a network slug, or `none` | `none` |
| `x.color` | a palette name or `#rrggbb` | `default` |
| `bluesky.color` | a palette name or `#rrggbb` | `sky` |
| `devnull.color` | a palette name or `#rrggbb` | `amber` |
| `threads.color` | a palette name or `#rrggbb` | `violet` |
| `mastodon.color` | a palette name or `#rrggbb` | `indigo` |
| `ui.detail` | `plain`, `minimal`, `medium`, `rich` (`standard` still parses as `medium`) | `rich`, or `plain` on an ESP or other low-resource terminal |
| `ui.layout` | `auto`, `single`, `split` | `auto` |
| `ui.theme` | `auto`, `native`, `black`, `white` or a name from `socli config themes` | `auto` |
| `ui.colors` | `full`, `mono` | `full` |
| `ui.density` | `compact`, `cozy`, `roomy` | `cozy` |
| `ui.icons` | `auto`, `nerd`, `unicode` | `auto` |
| `ui.images` | `auto`, `kitty`, `iterm`, `sixel`, `blocks`, `off` | `auto` |
| `ui.unfurl` | `on`, `off` | `on` |
| `ui.mouse` | `on`, `off` | `on` |
| `ui.enter` | `post`, `newline` | `post` |
| `colors.fg` | `#rrggbb` | `#e7e9ea` |
| `colors.dim` | `#rrggbb` | `#71767b` |
| `colors.line` | `#rrggbb` | `#2f3336` |
| `colors.surface` | `#rrggbb` | `#16181c` |
| `colors.blue` | `#rrggbb` | `#1d9bf0` |
| `colors.sky` | `#rrggbb` | `#6aa8e8` |
| `colors.teal` | `#rrggbb` | `#3cb8a6` |
| `colors.green` | `#rrggbb` | `#00ba7c` |
| `colors.sage` | `#rrggbb` | `#7db86b` |
| `colors.gold` | `#rrggbb` | `#d6b34a` |
| `colors.amber` | `#rrggbb` | `#d08a3c` |
| `colors.coral` | `#rrggbb` | `#ee6e4e` |
| `colors.rose` | `#rrggbb` | `#e5689a` |
| `colors.pink` | `#rrggbb` | `#f91880` |
| `colors.red` | `#rrggbb` | `#f4212e` |
| `colors.violet` | `#rrggbb` | `#a57fe0` |
| `colors.indigo` | `#rrggbb` | `#8480ff` |

## Colors

The `colors.*` keys take a `#rrggbb` value. A network color takes a palette name or a hex value, and the palette names are `default`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet` and `indigo`.

`socli config set bluesky.color teal` changes one network color.

## Environment

The environment overrides several settings: `SOCLI_NET`, `SOCLI_THEME`, `SOCLI_DETAIL`, `SOCLI_LAYOUT`, `SOCLI_IMAGE`, `NO_COLOR`, `NO_ICONS` and others. [CONVENTIONS.md](https://github.com/massimodeluisa/socli/blob/main/docs/CONVENTIONS.md#configuration-and-secrets) lists every variable and the order in which the flags, the environment and the file apply.
