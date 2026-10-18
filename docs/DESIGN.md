# socli design

socli draws every screen as text on a character grid. Each color comes from a named token, every post sits on a fixed set of columns, and a small set of glyphs stands in for icons. This document describes that visual system as the code draws it today: the palette in `src/theme.rs`, the grid in `src/render.rs`, `src/minimal.rs` and `src/chrome.rs`, the screens in `src/tui.rs`, `src/screens.rs` and `src/settings.rs`, and the mockups in `design/` that the screens were designed against. When a number here and the code disagree, the code is right and this page is out of date.

## Mockups and how they are made

Every screen exists first as an HTML mockup. `design/screen.html` renders one screen at a time from the sources in `design/src/`: `term.css` holds the palette and the cell size, `term.js` and `feedparts.js` hold the drawing helpers, `composer.js` holds the composer, and `design/src/screens/*.js` holds one file per screen (`ask`, `commandhelp`, `detail`, `draft`, `drafts`, `feed`, `filter`, `focus`, `help`, `image`, `keys`, `login`, `notifications`, `pick`, `post`, `profile`, `quote`, `settings`, `share`, `thread`). The page takes the screen and the theme from the query string, for example `screen.html?s=feed&theme=dark`. The [chrome refinement review](../design/improvements/chrome-refinement.html) compares the current terminal renderer across all four detail levels, light and dark themes, shortcut disclosure and the command palette. The mockup uses Geist Mono (the fonts are in `design/fonts`), a 9 px cell, a 22 px line height and a 100-column terminal.

`design/render.sh` loads each screen in headless Chrome at 1072 px wide with a device scale factor of 2 and writes `design/screenshots/<theme>/<nn>-<screen>.png`. The images are 1172 px high for `feed`, `thread`, `help`, `share`, `image`, `profile`, `focus`, `pick-*` and the `detail-*` screens, and 1084 px high for the rest. The screen list, in file-number order, is:

| No. | Screen | No. | Screen |
|---|---|---|---|
| 01 | `login` | 12 | `share` |
| 02 | `feed` | 13 | `image` |
| 03 | `thread` | 14 | `profile` |
| 04 | `post` | 15 | `quote` |
| 05 | `notifications` | 16 | `keys` |
| 06 | `ask` | 17 | `command-help` |
| 07 | `help` | 18 | `detail-plain` |
| 08 | `settings` | 19 | `detail-minimal` |
| 09 | `filter` | 20 | `detail-minimal-thread` |
| 10 | `draft` | 21 | `detail-rich` |
| 11 | `drafts` | 22 | `detail-rich-cards` |
| 23 | `pick-network` | 24 | `pick-detail` |
| 25 | `pick-theme` | 26 | `focus` |

The default themes are `dark` and `light`. Setting `THEMES="dark light mono"` renders the monochrome set as well. `design/index.html` is a gallery of the screenshots. The mockups define the intent. The code in `src/` is the implementation, and the last section lists where the two differ.

## Color

All color goes through the `Kind` enum in `src/theme.rs`. A screen never names an RGB value. It paints text with a `Kind`, and the active `Theme` turns the kind into a color. The kinds are `Fg`, `Dim`, `Bold` (the foreground color with the bold attribute), `Blue`, `Pink`, `Green`, `Red`, `Amber`, `Rose`, `Violet`, `Sage`, `Coral`, `Teal`, `Gold`, `Sky`, `Line`, `Link` and `Net(network)`. `Link` is blue in the full palettes and underlined in mono. `Net(network)` resolves to that network's accent color.

### Palettes

| Token | Dark | Light | Used for |
|---|---|---|---|
| bg | `#000000` | `#ffffff` | background |
| surface | `#16181c` | `#f2f5f6` | selection color, blended 40% into the background |
| line | `#2f3336` | `#d9e1e5` | rules, card frames, dividers |
| fg | `#e7e9ea` | `#0f1419` | body text, names |
| dim | `#71767b` | `#536471` | handles, times, labels, key descriptions |
| blue | `#1d9bf0` | `#1a8cd8` | links, mentions, verified mark, focus bar, bookmark |
| pink | `#f91880` | `#e0176f` | likes, the notification count |
| green | `#00ba7c` | `#00a36e` | reposts, saved and sign-in checks |
| red | `#f4212e` | `#d91c29` | an over-limit character count, errors |
| amber | `#d08a3c` | `#b36a1c` | dev/null accent, author color |
| rose | `#e5689a` | `#cc4a7f` | author color, likes in the minimal level |
| violet | `#a57fe0` | `#7e52c7` | Threads accent, author color |
| indigo | `#8480ff` | `#563acc` | Mastodon accent |
| sage | `#7db86b` | `#4c9a3a` | author color |
| coral | `#ee6e4e` | `#d24c2b` | author color |
| teal | `#3cb8a6` | `#13917f` | author color |
| gold | `#d6b34a` | `#a8841a` | author color, saved mark in the minimal level |
| sky | `#6aa8e8` | `#3a82cc` | Bluesky accent, author color |

These values are the same as the `--bg`, `--surface`, `--line`, `--fg`, `--dim` and accent variables in `design/src/term.css`. The stylesheet has two extra tokens that the code does not use: `--faint`, for the poll bars in the notifications mockup, and `--yellow`.

### Themes

Besides `black` and `white` (the two palettes above) and `native`, socli ships 27 named palettes adapted from opencode. The list is in `src/palettes.rs`, and `socli config themes` prints it. Sixteen of them have a light variant, and the other eleven are dark only. Some themes that opencode ships in both variants, such as `dracula`, `monokai` and `rosepine`, are dark only here: a light variant that would need five or more derived accents (see below) was left out.

| Theme | Variants |
|---|---|
| `aura`, `ayu`, `catppuccin-frappe`, `catppuccin-macchiato`, `dracula`, `matrix`, `monokai`, `nightowl`, `rosepine`, `vesper`, `zenburn` | dark |
| `carbonfox`, `catppuccin`, `cobalt2`, `everforest`, `flexoki`, `github`, `gruvbox`, `kanagawa`, `material`, `nord`, `one-dark`, `osaka-jade`, `palenight`, `solarized`, `synthwave84`, `tokyonight` | dark and light |

opencode defines a theme with a handful of roles. socli maps them onto its own palette tokens:

- opencode's background, text and primary become `bg`, `fg` and `blue`. `fg` is the theme's own text color, unchanged. Only `blue` is adjusted, to at least 3:1 contrast against the background.
- `surface` and `line` are the background mixed toward the text color, at 10% and 22% in a dark variant and at 7% and 16% in a light one.
- `dim` is the theme's muted text color. When that color has under 2.6:1 contrast against the background, or under 1.6:1 against `fg`, `dim` is the background mixed 55% toward `fg`.
- Each accent (amber, rose, violet, indigo, sage, coral, teal, gold, sky and the rest) is the theme color nearest in hue to the accent's target hue that has a saturation of at least 0.25 and 3:1 contrast against the background, used when it lies within 28° of the target. When no theme color qualifies, socli derives one at the target hue from the median lightness and saturation of the theme's accents and adjusts it to 3:1.

The palettes come from opencode, which is MIT licensed. [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) carries the credit and the license text.

### Native

`native` uses the terminal's own colors and leaves the background transparent, so the screen looks like the rest of the terminal. The reader writes the default foreground and background (SGR 39 and 49) in place of RGB, and writes accents as ANSI indexes that the terminal's own palette defines:

| Accent | Index | Accent | Index |
|---|---|---|---|
| red | 1 | coral | 9 |
| green | 2 | sage | 10 |
| amber | 3 | gold | 11 |
| blue, link | 4 | indigo | 12 |
| violet | 5 | pink, rose | 13 |
| teal | 6 | sky | 14 |

The selected row cannot be transparent. Its background is the terminal's background (from OSC 11) mixed 5% toward white on a dark terminal and toward black on a light one. When the terminal did not report a background, the row uses 256-color index 236 on a dark terminal and 254 on a light one. `dim` and `line` text use the terminal's faint attribute (SGR 2) instead of a gray, unless a `[colors]` override sets them, and the other overrides apply on top.

### Network accents

Each network has an accent that tags posts, chips and avatars when a feed mixes networks. The defaults are X `default` (the foreground color), Bluesky `sky`, dev/null `amber`, Threads `violet` and Mastodon `indigo`. The `[<network>] color` config key accepts one of 14 names (`default`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet`, `indigo`) or a `#rrggbb` value, and the network filter panel and Settings write the same key. The `[colors]` table replaces individual palette entries (`fg`, `dim`, `line`, `surface`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet`, `indigo`) with a `#rrggbb` value. An invalid value keeps the default and adds a notice.

### Author colors

An author's avatar box, rail and folds take one of eight colors, picked from the sum of the username's bytes modulo 8: amber, rose, coral, sage, violet, teal, gold, sky. The setting `feed.color_by` chooses what the rail and avatar follow. With `auto`, a view that mixes networks (and is not a single thread) colors by network and every other view colors by author. With `network` every post uses its network accent. With `author` every post uses its author color.

### Monochrome and plain

`ui.colors = mono` or `--mono` draws everything in one light gray (`#e6e6e6`) on black, with `line` at `#808080` and `dim` at `#8a8a8a`. Mono keeps a selected row's background and reverses it, so the selected row is black on gray. Links are underlined, because blue is gone. In mono, `Net` accents fall back to the foreground color. The palette matches the `.mono` block of `design/src/term.css`.

When `NO_COLOR` is set, the reader paints with the terminal's own colors and draws `Dim` with the dim attribute. The plain detail level (below) goes further: it uses no color at all, no dim, ASCII glyphs only, and a `>` beside the selected post instead of a background.

### Color depth and theme detection

The reader writes 24-bit color when `SOCLI_TRUECOLOR=1`, when `COLORTERM` is `truecolor` or `24bit`, when `TERM_PROGRAM` is `iTerm.app`, `vscode`, `ghostty` or `WezTerm`, or when `TERM` contains `direct` or `ghostty`. `SOCLI_TRUECOLOR=0` turns it off. Otherwise colors map to the nearest entry of the 256-color cube and gray ramp.

`ui.theme`, `--theme` and `SOCLI_THEME` choose the theme, in that order of precedence from the flag down to the file. The values are `auto`, `native`, `black`, `white` and the names listed under Themes. `dark` and `light` still parse and mean `black` and `white`. `auto` picks `black` or `white` from the terminal background.

The reader asks the terminal for its background color (OSC 11) at startup whatever the theme, because `auto`, `native` and the named themes all depend on the answer. It treats the background as dark when the luminance (weights 0.2126, 0.7152, 0.0722) is under 0.5. When the terminal does not answer, it falls back to `COLORFGBG` (a background of 0 to 6, or 8, counts as dark) and then to dark. A named theme follows that result: on a dark terminal it uses the dark palette, and on a light terminal it uses the light palette when the theme has one and the dark palette otherwise. A `-dark` or `-light` suffix (`nord-light`, `gruvbox-dark`) forces a variant. A `-light` suffix on a theme without a light variant is rejected.

## Grid

socli lays a post out on fixed columns so avatars, rails and stats line up from one post to the next. `src/render.rs` stores the columns as canvas columns. From 60 columns up, plain, minimal and rich sit inside a two-column margin on each side, so a terminal column is the canvas column plus 2. Medium has no side margin at any width, so its terminal columns are the canvas columns. This page gives terminal columns for the levels that keep the margin, counted from 0 at the left edge. In `--print` output for those levels the first two columns are the two spaces that every line starts with. A narrower terminal has no margin either, so its terminal columns are the canvas columns (see Narrow terminals).

| Element | Terminal columns | Canvas constant |
|---|---|---|
| Selection bar `▍` | 1 (not printed by `--print`) | 0 |
| Rich root avatar box (`╭───╮`) | 3 to 7 | `ROOT_AVATAR = 1` |
| Root rail `╎` and the fold circle | 5 | `ROOT_RAIL = 3` |
| Root text, header and cards | 10 | `TX = 8` |
| Elbow `├╌╌╌` or `╰╌╌╌` of a depth-1 reply | 5 to 8 | `ELBOW = 3` |
| Rich reply avatar box | 9 to 13 | `REPLY_AVATAR = 7` |
| Reply chain `╎` and the "Show N more replies" circle | 11 | `CHAIN = 9` |
| Reply text | 16 | `REPLY_TX = 14` |
| Stats in a feed row (reply, repost, like, bookmark) | 10, 18, 26, 34 | `STATS = [8, 16, 24, 32]` |
| Stats of the open thread's root | 10, 18, 26, 34 | the same `STATS` grid |
| Views and the `more` icon | right-aligned to the last content column | |
| Right margin | the last two columns | |

Medium keeps the rail, the elbow and the chain on those canvas columns. It has no side margin, so those marks are at terminal columns 3, 5 to 8, and 9. It draws no letter avatar on a post, so root text, cards and stats start at canvas column 5 and reply text at 11. The profile header still starts its text at column 8, beside the letter frame.

At `--width 100` the content spans columns 2 to 97, which is the length of the rule under the header (96 columns), and columns 98 and 99 stay empty. The usable width is the terminal width minus 4 from 60 columns up, and the whole terminal width below 60. The layout sets no minimum of its own, and the smallest terminal the reader draws in is 24 by 8 (see Minimum size).

The minimal and plain levels use a narrower grid: the network line at column 3, the text at column 5, and each reply depth adds 2 columns, down to a deepest depth of 2.

### Measured output

The columns above were measured from `socli --demo --print --width 100 --no-icons` output for every detail level and density, and counted from 0.

| Detail | Density | Avatar box | Rail | Root text | Reply box | Reply text | Stats columns | Printed lines |
|---|---|---|---|---|---|---|---|---|
| plain | compact, cozy, roomy | none | line at 3 | 5 | none (line at 5) | 7 | words, no columns | 87, 88, 88 |
| minimal | compact, cozy, roomy | none | line at 3 | 5 | none (line at 5) | 7 | words, no columns | 87, 88, 88 |
| medium | compact | none | 3 | 5 | none | 11 | 5, 13, 21, 29 | 89 |
| medium | cozy | none | 3 | 5 | none | 11 | 5, 13, 21, 29 | 105 |
| medium | roomy | none | 3 | 5 | none | 11 | 5, 13, 21, 29 | 116 |
| rich | compact, cozy, roomy | 3 to 7 | 5 | 10 | 9 to 13 | 16 | 10, 18, 26, 34 | 89, 105, 116 |

The line counts include the header, the chips row, the rule and the footer. They were measured with images off on 2026-10-21. In `--print`, rich keeps its letter avatars and medium leaves them out; inline images and link previews need the reader's image loader. The thread view (`socli --demo thread <id> --print`) keeps the same columns. With the two-column margin the open post's stats stay at 10, 18, 26 and 34 and the link icon sits at column 97 (measured on `socli --demo thread x:p1 --print --detail rich --width 100`; a dev/null root has only the reply cell at 10 and the star at 26). Medium has no margin and no post avatar, so the open post's stats sit at 5, 13, 21 and 29.

### Narrow terminals

Below 60 columns the reader changes the layout and keeps the detail level the user chose. The side margins go, so the content uses every column and the selection bar `▍` takes column 0. The footer leaves out the nav row and keeps the rule and the hint row. The sub bar's network chips collapse into one bold chip, `All` or the name of the one network in scope, and a click on that chip opens the network picker. The tabs after it narrow as described in Chips and tabs.

The next rules hold at every width and mostly matter in a narrow terminal. When the header's crumb doesn't fit, it is cut with `…`. The account label, the bell, or `back` stays on the right when it fits. When it does not, and a bold `[...]` fits at the right edge, that marker holds it. When a post's name and its `@handle · time · network` don't fit on one row beside the avatar, `@handle · time · network` moves to its own row under the name, and each piece is cut with `…` when it is still too long. The stats keep their grid columns while the canvas is wide enough for the text column, three 8-column cells and the 7 columns of the last cell. That is 39 columns when text starts at 8, and 36 in medium, where text starts at 5. A narrower canvas divides the remaining width after the text column into four equal action columns and draws icons without counts. Unsupported actions leave their column empty, so a Like control never moves into the Repost column. The same rule applies to feeds, threads, profiles and mentions, and the views readout stays hidden at these widths.

Link cards and rich link previews are as wide as their usual cap or the canvas width minus the columns before the text (8, or 5 in medium), whichever is less, and they keep their borders. A card that would be narrower than 16 columns becomes the link row that minimal draws, the display URL in dim at the text column. The profile card spans the canvas width at every size. Pickers, dialogs, the network filter, the composer and the drafts list use their preferred width or the content width, whichever is less, and a row that still doesn't fit, such as a key help description, is cut at the edge of the content. Minimal and plain keep their anatomy and use the real width. `--print` follows the same margin rule, so every printed line fits the `--width` it was given, margins included.

### Minimum size

The reader needs a terminal of at least 24 columns and 8 rows. In a smaller one it draws only `socli needs 24×8 · q quits` (`24x8` in plain), centered, and registers no click targets. `q`, `esc` and `ctrl-c` quit from there, and a resize back to 24 by 8 or more brings the normal screen back. `--print` lays a `--width` under 24 out at 24 columns.

### Densities

`ui.density`, `--density` and `SOCLI_DENSITY` choose between three spacings. The default is `cozy`.

| Density | Between posts, and after a root before its replies | Before a boxed card | Stats row | Rail under the avatar | Inline image height cap |
|---|---|---|---|---|---|
| compact | 0 blank rows | 0 | on the avatar's last row when the body is one line, no cards or replies | only when the post has more rows | feed 4, thread 6 to 22 |
| cozy | 1 | 1 | same as compact | same as compact | feed 6, thread 10 to 32 |
| roomy | 1 | 1 | always on its own row | always hangs under the avatar | feed 8, thread 12 to 36 |

Compact also drops the blank rows between a body and a card and between a root and its replies. The density changes rows only. Columns do not move. The minimal and plain levels keep one blank row between blocks at every density.

### Text safety

Post text, names, link cards and error messages come from the networks, so socli cleans them before it draws or prints them. Cleaning removes terminal control sequences (CSI, OSC, DCS and the other string sequences, in their ESC and C1 forms), every other C0 and C1 control, DEL, and the bidi embedding, override and isolate characters, and it leaves no marker where they were. A carriage return or a Unicode line or paragraph separator starts a new line in post text and becomes a space in one-line text such as a name, and a tab becomes four spaces. Zero-width joiners, variation selectors and the left-to-right and right-to-left marks stay, so emoji sequences and mixed-direction text keep their shape. The canvas then places text one grapheme cluster at a time: a wide character or an emoji ZWJ sequence takes the two cells the terminal gives it, a combining accent stays in its letter's cell, and a cluster that doesn't fit before the edge ends the write rather than being split. Wrapping measures the cleaned text and cuts a word longer than the line between clusters. `--json` output keeps the raw text.

## Detail levels

The detail level decides how much art the reader draws around the text. `--detail`, `SOCLI_DETAIL` and `ui.detail` set it, in that order of precedence. When none of them is set, the opening client decides: `rich` on a normal terminal, and `plain` when `TERM` or `TERM_PROGRAM` names an ESP client (`esp`, `espterm`, `esp32`, `esp8266`, `esp-idf`, `espressif`, or a suffixed name such as `espterm-256color` or `esp32s3`) or when `TERM` is `dumb`, `unknown`, `ansi`, `vt52`, `vt100`, `vt102`, `vt220` or `network` (`vt102-w` counts as `vt102`). `standard` still parses and means medium. In the reader, `d` steps to the next level in the order plain, minimal, medium, rich, and wraps from rich back to plain. `D` opens the detail picker (see Pickers) to choose a level directly. `d`, and `⏎`, `space` or a digit in the picker, write the level to `ui.detail` in the user config and the status reads `Detail: <level> · saved`. `s` in the picker writes the row under the cursor the same way. The next launch uses that value, after `--detail` and `SOCLI_DETAIL`. Narrowing the terminal never changes the level. Medium draws to the edges at every width. The other levels keep the two-column margin from 60 columns up.

A change of detail level, density, layout or terminal size keeps the reader's place. The selected post stays selected, and the first draw after the change scrolls so that the post's first line sits on the same screen row as before, as far as the list's scroll range allows. Scrolling follows the selection as usual after that draw.

| Level | What it draws |
|---|---|
| `plain` | Text only, in ASCII. No color and no dim; names stay bold. A `|` network line at column 3, a `>` bar beside the selected post, and counts in words. Dialogs and the composer use text without frames. |
| `minimal` | Text with a one-column network line `▎` in the network accent, a blank row between blocks, and counts in words (`5 replies  2 reposts  9 likes  1.2K views`, nonzero counts only). Likes show in rose, reposts in green, a saved post as `saved` in gold. No avatars, rails or cards. A quote sits behind its own line with a header and up to three text rows. |
| `medium` | Dotted thread rails and elbows, link and quote cards, media rows, icon stats and folds. Posts have no letter avatars, and the text starts one column after the rail. No side margin. |
| `rich` | Everything in `medium`, plus letter avatars and real images in the reader: profile pictures over the letter boxes, inline pictures for post media, and link previews with an image. |

Plain turns on ASCII mapping for every glyph (`●` and `☑` become `*`, `○ ◐ ☐` become `o`, `›` and `❯` become `>`, `│ ┃ ╎ ▎` become `|`, `─ ━ ╌` become `-`, corners and joints become `+`, `✓` becomes `+`, `✗` becomes `x`). It also writes key names as words (`⏎` as `enter`, `⌫` as `bksp`, `←` as `left`, `⇧⏎` as `shift-enter`). The unfocused pane's bar in split view is `:` in plain.

## Screen anatomy

Every list screen has the same five bands, from top to bottom.

1. The header is a bold `socli`, then a crumb: `›` in dim and each part in the foreground color, for example `socli › all › home`. The right side shows the account label in dim. A single-network scope shows `@handle`, or `not signed in` when that network has no session. The merged scope, including the demo, shows `Accounts`. In the reader that label is pressable: a handle opens that account's profile, and `Accounts` or `not signed in` opens the accounts page. When there are unread mentions it appends a bell icon and the count in pink (plain writes `N notifications`). When a thread or a profile is open in the reader, the right side shows `back` instead, with `⌫` revealed while Ctrl or Cmd is held. The printed header shows the label and never shows `⌫ back`.
2. The sub bar holds, in Home, the network chips on the left and the feed tabs on the right, described below. Other sections show `[Mentions]` (Notifications), `Search ❯ input` or `Search  <query>`, `Posts  @handle` (a user's posts), or in a thread `Post` on the left and `Replies oldest first` or `Replies newest first` on the right.
3. The rule is a row of `─` across the content width in the `line` color, on terminal row 4 (the reader uses it as the top of the body).
4. The body is the posts, a thread, a settings list or a form. On a list, one blank row under the rule and one blank row above the footer rule belong to the scrolled text, at every density. At rest they pad the list, and a scroll can put a post on the first and last body rows. When the selected post is taller than the pane, `j` and `k`, the wheel and the page keys move through that post until the far edge is on screen, and the next press leaves it. Settings, help and a profile have no pad.
5. The footer is a rule, the nav row, and the hint row. Below 60 columns the footer leaves out the nav row. Printed output keeps a blank row before that rule.

### Chips and tabs

The merged scope shows network chips on the left and `[Home]  Discover  Saved` aligned to the right edge. `[All]` is bold when all networks are on and dim otherwise. A chip is a dot and a network label: the dot is filled (`●`) when the network is on and empty (`○`) when it is off, colored in the network accent when the network is signed in and dim otherwise. On chips are drawn in the foreground color when All is on and bold when it is not. Off chips are dim. A single-network scope shows one bracketed bold chip, `[● X]`. A dim `│` separates the chips from the tabs `Home`, `Discover` and `Saved`: the open tab is `[Home]` with bold brackets and a dim digit, the others are dim. Digits show for the first nine tabs. When the row is too narrow, the reader narrows the row in this order: off chips drop from the right, on chips shrink to their network's initial (`● X  ● B  ● d  ● T  ● M`), off tabs drop, tab numbers drop, and finally on chips that are not bracketed drop. Dropped chips and tabs stay behind a bold `[...]` on that side of the row when the marker fits. Pressing it opens a small list of those controls: arrows, `j` and `k` move, Enter or a click runs the control, and Esc or a click outside closes the list. A chip that only shrank to its initial stays on the row and is not in the list. Below 60 columns the single chip still opens the network picker, and `[...]` is not a second copy of the networks that chip replaced. A bracketed chip never shrinks. At width 80 (`--demo --print --no-prompt --width 80`) the row reads `[All]  ● X  ● B  ● d  ● T  ● M  │  [1 Home]  2 Discover  3 Saved`. Holding Ctrl or Cmd reveals each tab's number without changing its click action.

### Nav and hints

The suggested-action footer always pairs a bold foreground key with a muted action label. Elsewhere, shortcut labels, including tab numbers, `n` for New post, and keys in the composer and dialogs, appear only while a reported Ctrl or Cmd press is held, and visible interactive targets are underlined for that same span. socli does not ask the terminal to encode every key as an escape sequence, because that mode replaces a composed character with the key that produced it, so Ctrl and Cmd presses are not reported and the labels stay hidden. Shift keeps its normal role in uppercase letters, shifted symbols and combinations such as Shift-Enter. Command-modified events are not interpreted as text or unmodified app actions, except Cmd+R, which reloads the page on screen. Terminal-owned shortcuts such as Cmd+C remain with the terminal. A reported release hides the labels, and losing terminal focus clears them too. The shortcuts continue to work and `?` opens their full reference.

The movement hint uses `↑ ↓ ← →` (`^ v < >` in plain). Suggestions follow the page, the selected post's network capabilities and the focused control. Focusing a link, for example, offers Open link first. Help remains at the right edge, with status text using the room between the suggestions and Help. The key combinations below describe the available actions. Suggested footer actions keep their keys visible in both Ctrl/Cmd states. Printed output retains its key reference because it has no held-key state.

The nav row is a bold `socli›` followed by `Home`, `Notifications` (the unread count in pink) and `Search`. The current section is bracketed and bold, `[Home]`, and the others are dim. New post sits at the right edge; its dim `[n]` appears while Ctrl or Cmd is held. When `new post` does not fit, a bold `[...]` at the right edge holds it and opens the same list.

The hint row lists keys as a bold key and a dim label. The hints depend on the screen and on what the selected post's network can do (`r` needs posting, `l` likes, `t` reposts, `b` bookmarks, `v` needs media), so in the reader a hint that would do nothing is not shown (on a dev/null post the row reads `l star` and has no `t repost` or `b`, and on a Mastodon post the `l` hint reads `l favorite`). `--print` has no selected post, so its footer lists every key.

| Screen | Hints, in order |
|---|---|
| Feed | `j/k move`, `⏎ open`, `v view`, `u profile`, `r reply`, `l like`, `t repost`, `s share`, `f search`, `a network`, `? help` |
| Thread | `j/k move`, `r reply`, `l like`, `t repost`, `b bookmark`, `⌫ back`, `u profile`, `s share`, `? help` |
| User | `j/k move`, `⏎ open`, `s share`, `o open in browser`, `⌫ back`, `? help` |
| Notifications | `j/k move`, `⏎ open`, `r reply`, `l like`, `? help` |
| Search | `⏎ search`, `esc cancel` |
| Inspect (`.`) | `j/k move`, `⏎ expand`, `⌫ back` |
| Settings | `j/k move`, `space toggle`, `h/l change`, `esc close` |

A status that stays up, such as a post count or `loading`, sits before Help in the hint row. It takes at most a third of the row and is cut with `…` when it is longer. A confirmation or an error replaces the whole hint row for as long as it shows, and the key suggestions come back when it goes. A saved notice never shows the config file's path, and `socli config path` prints it. Suggestions are added from the left while they fit, and Help keeps its place on the right. A hint that does not fit stays behind a bold `[...]` immediately before `? help`, or after the last hint when that row has no help key. It opens the same list. A message that confirms an action, such as `liked` or `saved`, goes away 4 seconds after it appears, and the full hint row comes back without a key press. An error stays until the next key, click or paste, and a status that press sets replaces it. Other messages, such as progress, stay until something replaces them.

### Loading

A request that runs longer than 150 ms replaces the hint row with a loading line. The rich level shows `◐ loading` with the glyph turning through `◐ ◓ ◑ ◒` every 120 ms. Every other level shows `loading…` (`loading...` in plain). A send shows `sending`. A rate-limited network shows `<Network> rate limit reached; try again in N minutes` from 120 seconds up and `try again in Ns` below that. Keys pressed while the line is up are dropped. `ctrl-c` quits with exit status 130. If the terminal goes away, or a SIGHUP or SIGTERM arrives, while the reader can't react, a watchdog ends it two seconds later with exit status 129, or 143 after SIGTERM. While a send is in flight the first `ctrl-c` is held and the line adds ` · ctrl-c again to quit`.

Reaching the 10th post of the current page in a feed, search, profile, or mentions list draws a loading card under that list and fetches the next page. A page shorter than 10 posts starts that fetch on its last post. Each signed-in network is asked for at least 20 posts. Medium and rich draw a box at the text column with `loading…`. Minimal and plain write the words only (`loading...` in plain), with no box. The card is in the frame before the request starts, because a slow load only repaints the footer. The request runs off the input thread, so `j`, `k`, and the wheel keep moving. One visit to that part of the page fetches once. Moving back before the 10th post and coming forward tries again. `m` loads one page the same way. `ctrl-r` and `cmd-r` reload the current page from the start, the same as `:refresh`, from every view. `r` alone still replies. An open thread reloads that thread. A thread has no next page. The demo does not fetch.

## Split view

On a wide terminal the reader shows the feed on the left and a thread on the right. `--layout`, `SOCLI_LAYOUT` and `ui.layout` choose `auto`, `single` or `split`, and `auto` is the default. `auto` splits when the terminal is at least 124 columns wide. A forced `split` works from 100 columns. Below 100 columns the reader uses one pane whatever the setting says. Split view applies to Home, Notifications and Search, not to Settings or a profile, and it stays on while the composer, the command line, the filter panel, search or the share dialog is open.

The content width is divided as `left = (width - 4) / 2` and `right = width - 4 - left`, with a vertical `│` divider in the `line` color between them from the row under the rule to the end of the body. The rule above the panes gets a `┬` where the divider starts, and the rule above the footer gets a `┴` where it ends (the `┴` is not drawn while the composer is open).

`⏎` or `→` gives the keys to the right pane, and `⌫`, `←` or `esc` gives them back to the feed. Neither key replaces the feed: the other posts stay on the left. A click or a wheel notch over a pane also moves the keys there (see Focus and mouse). The pane with the keys has a blue `▍` bar and the selected row's `surface` background. The other pane keeps its selection but draws the bar in the `line` color and drops the background, so the selection shows where it will return. The right pane loads that conversation's replies and shows them before the keys move. Below the split width, `⏎` still opens the thread on the one pane, and `⌫` returns to the feed. Its sub bar reads `Post` and `Replies oldest first`. With nothing selected it shows `Select a post to read it here` in dim.

## Post anatomy

A post in the medium and rich levels has these parts, top to bottom.

The repost line reads `<repost icon> @by reposted`, dim, at the text column.

The header holds the author's name in bold, the verified icon in blue when the account is verified, `@handle` in dim, ` · <age>` in dim (a link to the permalink), and in merged feeds ` · <network label>` in the network accent. The `more` icon sits dim at the right edge.

The avatar box is `╭───╮`, `│ L │` with the author's initial in bold, `╰───╯`, in the author or network color. The box joins to the rail with `╭─┴─╮` above and `╰─┬─╯` below when the thread continues. In the rich level with a graphics protocol, a profile picture replaces the letter: the reader crops it to the largest square that fits the 5 by 3 cell box, centers it in the box and masks it to a circle (a rounded square with a corner radius of a fifth of the side for X business accounts), then paints it over the letter box. Terminals without image transparency (sixel and half blocks) get the corners filled with the theme background.

The body is text wrapped at the text column. A post in a list shows at most three of those lines, and a longer one adds `Show more` (or `Show less` once it is open) after the stats and before a fold. The open thread shows the whole body. Links and mentions use the `Link` color.

The link card is a four-row box in the `line` color, at most 58 columns wide and never wider than the width left of the text column. The first row is the link icon and host in dim, the second row is the page title in bold followed by its description in dim, or the displayed URL when the page has no description.

The quote card is the same box. The first inner row is the quoted author's name in bold with `@handle · age` in dim, the second is the first line of the quoted text.

The media row, in dim, shows the image icon, then `image · <alt text>` or `2 images`, ending with `view` when images are enabled, with `v` revealed while Ctrl or Cmd is held.

The stats row has up to four cells at the stats columns: reply, repost, like and bookmark, each an icon followed by a count. The count is left out on networks that do not report it (the icon stays). X and Bluesky draw all four cells, and so does a signed-in Mastodon account, which reports counts but no views. Threads draws only the reply and repost icons. dev/null draws the reply cell, leaves the repost column empty and uses the like column (16 columns after the text column), with no bookmark cell. A reposted post shows the repost cell in green, a liked post shows the liked icon and count in pink, a bookmarked post shows the bookmarked icon in blue (the bookmark cell never has a count). Each action has a seven-column target, and the next action begins eight columns later, leaving a one-column gutter between targets. The grid is shared by feed, profile, mention and thread-root rows. Counts that exceed their target are truncated at a grapheme boundary. Views sit at the right edge as the views icon and an abbreviated count (`4K`, `870`) when the network reports views and the count is above zero, provided at least two columns separate them from the last action target. A narrow row keeps its actions and leaves out the views readout. The thread root also leaves out its trailing link icon when that spacing is unavailable. In cozy and compact, when the body is a single line, the stats share the avatar's last row. In roomy they always get their own row.

A fold under a root whose replies are not loaded reads `○ Show this thread`, with the circle on the rail and the label at the text column. `○ Show N more replies` sits at the chain column with its text at the reply text column.

### Rich images and link previews

These elements appear only in the rich level inside the reader, never in `--print`, and only after the images have loaded.

- An inline image is placed at the text column, with a width of at most 58 columns and the width left of the text column. The height comes from the picture's aspect ratio and the terminal's cell shape. A feed post caps that height at 4, 6 or 8 rows by density. An open thread starts at 6, 10 or 12 and, on a tall pane, grows to two fifths of the pane, at most 22, 32 or 36. The page uses about three fifths of the pane (at least 12, 16 or 18, and at most 28, 40 or 48) and narrows a picture that would still be taller, so the box keeps the picture's shape. While the picture loads, and while a background thread encodes it for the size of its box, the reader draws a frame (`╭─╮`, `│`, `╰╯` in `line`) with `image` centered in dim, when at least 3 rows and 2 columns are free. Below it a dim caption reads `1 of N · v view`, or `video · o plays it in the browser` for a video or GIF.
- A rich link card is up to 64 columns wide. It falls back to the four-row link card when fewer than 28 columns are available. A page image of at least 400 px width and an aspect ratio of 1.3 or more (or of unknown size) is a large card: the image fills the top (up to 18 rows, centered) and the bold title and dim site name sit below it. Any other image is a thumbnail of 12 columns by 6 rows at the left, with the title (up to two bold rows), the description (up to two dim rows) and the host to its right. The loading label is `loading preview`. When a link has no image from the network and `ui.unfurl` is on, the reader fetches the page and reads its preview image (the fetch reads at most 512 KB of the page head, times out after 5 seconds and follows at most 5 redirects, and an image downloads up to 8 MB).

## Thread view

Opening a post draws the root with the standard anatomy plus a date line (`6:42 PM · Oct 20, 2026`, a link to the permalink, with ` · N views` appended when the network reports views), a rule in the `line` color, and a stats row at the thread columns with the link icon at the right edge. In the roomy density the handle moves to its own row. Replies come after one blank row. Depth-1 replies hang from the root's rail with `├╌╌╌` elbows (`╰╌╌╌` for the last one) in the root's rail color. Replies deeper than one level use the chain at column 9 in medium and at column 11 when the side margin is on. Medium reply text starts at column 11, one column after that chain. A reply's header carries the liked and bookmarked marks at its right edge. The sub bar reads `Post` (bold when the thread has focus, otherwise dim) and `Replies oldest first` or `Replies newest first`, which `feed.replies` and Settings choose. `⌫`, `←` or `esc` returns to the feed.

## Notifications

Notifications is a list of the same post rows with a bell count in the header and `[Mentions]` in the sub bar. In the minimal level a mention row adds `mentioned you` in blue after the header. `⏎` opens the conversation, and the footer offers `r reply` and `l like`.

## Profile view

`u` opens the author of the selected post, and `:user @handle` opens an account on the selected network. Standard and rich show a thin frame titled `Profile`, with the network at its right edge. Inside it, the 5 by 3 avatar starts at canvas column 2, and the name, handle and bare profile URL occupy separate rows at canvas column 8. That is terminal column 10 when the side margin is on, and column 8 in medium. Medium keeps that column, because the letter frame is still drawn. The verified mark stays beside the name. The biography wraps to at most 76 columns and keeps its paragraphs, followed by Open profile and Share profile, whose `o` and `s` keys appear while Ctrl or Cmd is held. These actions are mouse targets and Tab stops; sharing always targets the displayed account, even when a post is selected. Only the frame's title row receives the profile selection background.

A `Posts` section separates the identity from the timeline. Its counts describe loaded posts and loaded posts with media, and the latest loaded timestamp appears when there is room. These are not account totals. With fewer than 24 body rows, the frame removes its extra blank rows and the posts section drops its second rule, keeping the first post visible in the 60 by 20 demo. Rich image previews use at most 8, 6 or 4 rows at roomy, cozy or compact density, and at most 4 rows in a short window. `v` still opens the image viewer.

Minimal and plain keep the account details, wrapped biography, profile actions and loaded post count, using their network line instead of a frame or avatar. Plain uses ASCII for the interface and the terminal's own colors. The reader retains the profile returned by the provider, including on an empty timeline, and later post authors without a biography do not erase that biography. In demo mode, a repost belongs to its reposter's timeline; opening River's account therefore shows River's original post once.

The reader has no screen for your own profile yet. The [interactive profile comparison](../design/profile.html) and [native recordings](../design/profile-evidence/index.html) provide the visual reference.

## Composer

`n` opens the composer above the footer. Standard and rich use a bordered box drawn in blue: `╭─ New post ──╮`, with `│` sides. Plain and minimal leave out the frame. The title is `New post`, `Replying to @handle`, `Quote post` or `Share profile`. Quoting and sharing add a quote card (64 columns wide, `line` frame, bold name, dim `@handle · Network`, two text rows) and a `to ● X ● Bluesky` row. The text area is two to six rows, and the full-screen composer fills the body. The bottom edge is `╰──── <gauge> <chars left> ─╯`.

The gauge is one of eight circle glyphs that fill as the text grows, measured against the `max_chars` of the strictest network in the target list. The count is dim, and red once it goes negative. The hint row names the send key and the newline key that `ui.enter` sets. With `post`, the default, it reads `⏎ post` (or `⏎ reply`) and `ctrl-j newline`, or `⇧⏎/ctrl-j newline` in terminals that report the kitty keyboard protocol. With `newline` it reads `ctrl-s post` and `⏎ newline`, and the send key becomes `ctrl-s/ctrl-⏎` in those terminals. `ctrl-o drafts N` follows when drafts exist, then `esc cancel`. At the right the row names the destination, `posting to X as @handle`, `replying on <network>` or `posting to X, Bluesky and Threads`, and a status message comes after it, cut to the columns that are left, so the destination stays in view. With a single target, Tab and Shift-Tab cycle the network, and the post goes to the network the row shows.

`ui.enter` decides what `⏎` does in the composer. With `post`, `⏎` sends and `ctrl-j` (or `⇧⏎`) adds a line. With `newline`, `⏎` adds a line and `ctrl-s` sends, as does `ctrl-⏎` where the terminal reports it. The `Enter in composer` row in Settings changes the value, and no flag or environment variable sets it.

The text field edits whole graphemes, so an accented letter, a flag or a family emoji moves and deletes as one character. The cursor column is the display width of the text before it on its line, so wide CJK characters and wrapped rows still put it in the right cell. `←` and `→` move one grapheme and, with `alt` or `ctrl`, one word. `alt-b` and `alt-f` move one word too, which is what Option-Left and Option-Right send when the terminal encodes them as `esc b` and `esc f`. `Home` or `ctrl-a` and `End` or `ctrl-e` go to the start and end of the line, and `↑` and `↓` keep the column on the line above or below. `⌫` deletes back and `Delete` forward, `alt-⌫` or `ctrl-w` deletes the word before the cursor, and `ctrl-u` deletes back to the start of the line. `ctrl-z` undoes and `ctrl-y` or `ctrl-shift-z` redoes, up to 100 steps. Typed characters up to and including a space count as one step, and a cursor move or any other edit ends the step. Search, ask and the command line take the same keys except `↑` and `↓`, which keep their own meaning there. In ask, `ctrl-u` keeps scrolling the transcript up, paired with `ctrl-d`, so it deletes nothing there, while `ctrl-w` and `alt-⌫` still delete a word. `ctrl-k` is not bound.

A paste arrives as one bracketed paste. The composer inserts it at the cursor as a single undo step, after turning `\r\n` and `\r` into newlines and a tab into four spaces and dropping every other control character. Search, ask and the command line take the same text with each newline turned into a space. A paste never sends, runs a command or presses a key, and the reader drops it in every other mode and while a composer dialog is open.

Drafts autosave every 3 seconds. `esc` or back with unsaved text opens the question box `Unsaved draft` (45 columns wide at row 5) with `Would you like to save the draft?` and the keys `y save   n discard   esc keep writing`. After a save the status reads `draft saved · ctrl-o in the composer opens your drafts`. `ctrl-o` opens the drafts list: an 84-column box with 8 rows, each showing the age (column 2), the target (column 13, up to 19 columns), the network (column 33) and the first line of text (column 43, up to 38 columns). Its keys are `j/k move`, `⏎ load`, `d delete` and `esc close`.

### Drafts and sending

A send keeps its draft until socli knows what each network did with it. Before the first network call socli writes the text as typed, the targets and the reply or quote into the draft, together with a `sending` list of the networks it is about to reach, and it sends nothing when that write fails. When every network takes the post the draft goes away, and otherwise the same draft stays with only the networks that failed. A draft that still carries `sending`, because socli stopped mid-send or could not update the draft afterwards, opens with the status `last send may have reached <networks> · check before posting again`, and `socli drafts` shows `may have posted` in its target column. socli never retries a send on its own. A draft counts as changed when its text, targets, reply or quote differ from what was loaded, so switching the network alone autosaves, and every draft write holds a lock on `drafts.json.lock` so two socli processes cannot lose each other's drafts.

## Share dialog

`s` opens a centered box, `╭─ Share post ──╮` or `╭─ Share profile ──╮` in the `line` color with a bold title and a width of 64 columns (or the terminal width minus 8, never under 24). Each row has a label at column 3, a value or a hint, and its key letter dimmed at the right edge. The focused row has a blue `▍` at column 1.

| Row | Key | Notes |
|---|---|---|
| `Copy link` | `s` | the bare URL shows dim and linked |
| `Copy profile link` | `p` (on a profile dialog, `s`) | |
| `Repost on <network>` | `t` | reads `Undo repost on <network>` after a repost, and `Reposted on <network>` when the network cannot undo |
| `Quote on` or `Share to` | `q` | `Quote on` when the post's own network quotes natively, `Share to` for other networks |

With more than one signed-in posting network, the quote row shows checkboxes (`☑ X  ☐ Bluesky`, the checked ones in blue). `←` and `→` (or `h` and `l`) move between them and `space` toggles one. At least one stays on (`keep at least one network on`). The shared post's network comes first. A checkbox that does not fit before the key letter continues on the next row, starting at the value column. With all five networks signed in, the 64-column dialog draws `☑ Threads  ☐ X  ☐ Bluesky  ☐ dev/null` on the `Quote on` row and `☐ Mastodon` on a second row (the order shown is for a shared Threads post). The hint row reads `⏎ choose  space toggle  esc close`, or `⏎ choose  esc close` when there is nothing to toggle. Every action closes the dialog, and `esc` or `⌫` closes it without acting. `s` pressed again inside the dialog copies the link.

## Pickers

`A` and `D` open a picker (and `:theme`, or `⏎` on the Theme row of Settings, opens a third), a centered box drawn like the share dialog: the same width (64 columns, or the terminal width minus 8, never under 24 and never wider than the content width), a rounded frame in the `line` color, and a bold title (` Network `, ` Detail ` or ` Theme `) set into the top edge. Each choice is one row. A radio mark sits at column 3 (`●` on the current choice, `○` on the others), the label at column 5, the value in dim at column 22 (or two columns after a longer label, cut short so it ends before the key), and the digit that picks the row, dim, at the right edge. The current row's label is bold. The row under the cursor has a blue `▍` at column 1 and a highlighted background. The hint row reads `⏎ choose  esc close`. The box opens with the cursor on the current row, and on the first row when no row is current. A list too tall for the terminal scrolls: the box shows the rows that fit (at least three) in a window around the cursor, and the hint row ends with the cursor's true position, as in `⏎ choose  esc close  8/31`.

`j`, `↓` and `tab` move the cursor down, `k`, `↑` and `shift-tab` move it up, and both wrap. A digit picks the row that shows it, and other digits do nothing. `⏎` or `space` picks the cursor row. `esc`, `⌫`, `←` and the key that opened the picker close it without a change. Any other key is ignored. Picking the row that is already current only closes the box. With the mouse on, hovering a row moves the cursor there, the wheel moves it one row per notch, a click picks the row, and a click outside the box closes it like `esc`.

| Picker | Rows | Picking a row |
|---|---|---|
| Network (`A`), digits `0` to `5` | `All networks` (the value lists the signed-in networks, comma separated), then X, Bluesky, dev/null, Threads and Mastodon, each with `@handle` or `not signed in` and a mark in the network's accent | `All networks` shows every signed-in network, a network shows only that one |
| Detail (`D`), digits `1` to `4` | `plain` (`text only, ASCII, no color`), `minimal` (`a network line, counts in words`), `medium` (`rails, cards and icon stats`), `rich` (what rich draws in this terminal: `Images · kitty`, `Images · iTerm2`, `Images · sixel`, `Images · blocks in this terminal`, or `Images · this page` in the browser, and `Images off · ui.images` when images are off), with a blue mark | writes the level to `ui.detail`, with the status `Detail: <level> · saved`. `s` writes the cursor row the same way |
| Theme (`T`, `:theme`, or Settings), no digits | `auto`, `native`, `black`, `white`, then the 27 named themes, each with its description in dim (`dark` or `dark and light` for a named theme) and a blue mark | moves previews live: the screen behind the box takes the theme under the cursor. `⏎` or `space` keeps it and saves it to `ui.theme`, and `esc`, `⌫` and `←` restore the theme the box opened on |

A network that is not signed in has a dim label. Picking it keeps the box open and shows `<label> is not signed in. Run socli login <slug>` in the status. `All networks` is current when every signed-in network is on, and a network row is current when it is the only one on. `D` opens the detail picker from Settings too, over the Settings screen. The Theme picker saves only when the pick differs from the theme it opened on. A network row is current only when more than one network is available, so with a single signed-in network only `All networks` is current.

## Command line

`:` or `ctrl-k` opens a centered command palette in medium and rich. The palette is at most 76 columns wide, with a title, input, matching commands and clickable Complete, Run and Close actions. Suggestions scroll to the selected row when the terminal is short; an empty result shows `No matching commands`. Long input scrolls by grapheme to keep the cursor inside the field. Clicking outside closes the palette.

Plain and minimal keep command entry at the bottom, with matching commands above it and no dialog frame. `tab` completes, `⏎` runs the typed or highlighted command, and `esc` closes. The footer reveals these key names while Ctrl or Cmd is held. `:help` opens the key reference and `:quit` leaves. `T` or `:theme` opens the theme picker, and `:theme <name>` applies the named theme. The command registry in `src/command.rs` supplies the same commands to both presentations.

## Network filter

`F` opens a 42-column panel at column 2, starting on the rule row. The title is `╭─ Networks ──╮`, with the rule continuing to the right on that row. Each of the five rows has a blue `▍` for the cursor, a dot in the network accent (column 5 of the panel), the network label and the account handle, or `not signed in`. The footer offers Toggle, All, Save and Close; their key labels appear while Ctrl or Cmd is held. Toggling a network that is not signed in shows `<Label> is not signed in. Run socli login <slug>`. The last network on cannot be turned off. `0` shows all networks and `<` or `>` steps through one network at a time. `a` steps the scope through All, then each signed-in network in network order, then back to All. From a partial set of networks (two or three on) it goes to All, and with fewer than two networks signed in it does nothing. `ctrl-a` still opens ask. `A` opens the network picker (see Pickers).

## Settings

`,` opens Settings, a list with a blue `▍` cursor at column 0. A value shows as `‹ value ›`, `space` toggles and `h` and `l` change it. Every change applies at once and is written to the config, and a status row above the rule shows a green check and `saved`. `socli config path` prints where the file is.

In `--demo` the reader never writes the config. A change still applies for the session and the Settings screen shows the new value, but the status row reads `<key> = <value> · demo, not saved`, for example `ui.density = compact · demo, not saved`. The theme picker, `:theme <name>`, the network filter's `⏎`, `d` and the detail picker follow the same rule, with `theme <name> · demo, not saved`, `N networks in your feed · demo, not saved` and `Detail: <level> · demo, not saved`.

| Section | Rows |
|---|---|
| Networks | `Accounts` (opens the accounts page), then for each network `● X  @handle` or `not signed in`, the word `login` or `logout`, and `color ● <name>` |
| Feed | `Color posts by` (auto, network, author), `Default network` (used by `socli user` and the composer), `Replies order` (oldest, newest) |
| Appearance | `Detail`, `Layout`, `Theme`, `Colors`, `Density`, `Icons`, `Images`, `Fetch link pages`, `Mouse`, `Enter in composer` |

Text that does not fit in its column is cut with `…` at a grapheme boundary. A long handle stops before `login` or `logout`, and a value, description, color name or font line stops before the next column or the right edge. `login` and `logout` stay in their column, which is also where a click hits them.

`h` and `l` step the Theme row through the list, with a live preview and a save, and `⏎` on it opens the theme picker. `⏎` on `Accounts` opens the accounts page. `⏎` on a network row signs that network in or out, and so does `space` when it is signed out. `space` on a signed-in network still toggles it in the merged feed. A click on the word `login` or `logout` does the same as `⏎`. Under the rows are the lines `Font and size`, `recommended: GeistMono Nerd Font`, `brew install --cask font-geist-mono-nerd-font`, and `Palette  socli config set colors.<name> #rrggbb`. The `Font and size` line names the setting of the terminal in use (`terminal.integrated.fontFamily` in VS Code and Cursor, `font-family` in Ghostty, Settings › Profiles › Text in iTerm2 and Terminal, `config.font` in WezTerm). The font and its size belong to the terminal. socli does not set them.

The accounts page lists the five networks. Each row shows the handle or `not signed in`, and `login` or `logout`. `⏎` or `space` runs that action inside the reader: socli runs the same sign-in or sign-out as `socli login <slug>` and `socli logout <slug>`, then reloads the accounts. Sign-in instructions stay in the reader's temporary terminal screen. The crumb reads `settings › accounts`. `esc` returns to the Settings list on the `Accounts` row. In `--demo` the action applies nothing and the status reads `<Network> · demo, not saved`. `socli login` and `socli logout` on the command line keep working.

## Key help

`?` opens the key help overlay, a list of every key in five groups (Move, Act, Go, View, Other). The group label is bold at the left, the key follows at 12 columns in, and the description is dim at 30 columns in. The overlay uses two columns when the width is at least twice the widest description plus 30, and one column otherwise. The keys:

| Group | Keys |
|---|---|
| Move | `j k` or `↓ ↑` move, `space` or `PgDn` next page, `PgUp` previous page, `ctrl-d` and `ctrl-u` half a page, `g G` or `Home End` first and last, `⏎` or `→` open, `⌫`, `←` or `esc` back |
| Act | `l` like (again to undo), `t` repost (again to undo), `b` bookmark (again to remove), `r` reply, `n` new post, `o` open the link, `v` view images, `u` author's profile, `s` share, `m` load more, `ctrl-r` or `cmd-r` reload this page, `.` inspect the post as JSON |
| Go | `1` to `9` feed tabs, `{` and `}` previous and next feed tab, `[` and `]` previous and next section, `tab` and `shift-tab` the next and previous control, `:` or `ctrl-k` command line, `f` or `/` search, `ctrl-a` ask |
| View | `d` next detail level, `D` pick the detail level, `a` next network and then all, `A` pick a network, `F` filter networks, `0` all networks, `<` and `>` one network at a time, `,` settings, `⌘-click` or `ctrl-click` opens names and links in the browser |
| Other | `?` this help, `q` quit |

## Focus and mouse

`tab` and `shift-tab` move a focus ring through the controls on the screen, and `⏎` presses the control under the ring. With the mouse on, a left click presses the same controls, the wheel moves the selection or a cursor, and hovering moves the cursor of a dialog. Both work from one list of targets that `draw` in `src/tui.rs` rebuilds on every frame, so a control that the frame does not draw, or that a dialog covers, cannot be focused or clicked.

### Targets and order

The targets sit in five zones, and the zones set the tab order. The header starts with clickable breadcrumb segments, followed by Back when it is shown, and otherwise the bell (`N notifications` in plain). The account label is not a target. The sub bar holds the network chips and feed tabs, whose numbers appear while Ctrl or Cmd is held. The body holds, for every post it draws, the avatar box in rich (the ring covers its letter row), the name and handle, the time, each URL, mention and hashtag in the text, the quote card, each link preview card, the media row or the inline image, each stats cell, and the fold rows such as `○ Show this thread` and `Show 2 more replies`. A profile adds its links. The minimal and plain levels register the same kinds of targets on what they draw, so the stats cells there are words such as `5 replies` and `86 likes`. In split view the left pane comes before the right pane. The footer holds the nav sections and New post, whose `n` appears while Ctrl or Cmd is held. Each hint pair in the hint row is a target for the mouse only: a click on `r reply` runs `r`, and `tab` skips the hints.

`tab` visits the targets by zone, then by row, then by column, and wraps from the last target to the first. With no ring on the screen, `tab` goes to the first header target and `shift-tab` to the last target, New post.

| Target | `⏎` on the ring, or a click |
|---|---|
| `⌫ back` | goes back, like `⌫` |
| Account label, bell, nav section | opens that section; `Search` opens the search input, like `f` |
| `[n] new post` | opens the composer, like `n` |
| Network chip | shows that network, or all of them for `[All]`, like the same row in the network picker |
| Feed tab | opens the tab, like its digit |
| Avatar, name and handle | opens the author's profile, like `u` |
| Time | opens the thread, like `⏎` |
| URL, profile link | opens the address in the browser |
| Mention | opens that user's posts on the post's network |
| Hashtag | searches for the tag, `#tag` |
| Quote card | opens the quoted post's thread |
| Link preview card (title row) | opens the card's URL in the browser, like a URL in the text |
| Media | opens the image viewer, like `v` |
| Reply, repost, like and bookmark cells | `r`, `t`, `l` or `b` on that post |
| Fold row | shows the folded replies or opens the thread, like `⏎` on that row |
| Hint (mouse only) | runs the hint's key |

A target that belongs to a post selects that post first, so the action applies to it. A link preview card is a Link target for its URL. Its title row is the `tab` stop, and its other rows respond to a click only, as the rows of a quote card do.

### The ring

The focused target is bold and underlined. Colored themes also use blue text; monochrome, plain and native themes retain their text color. The ring shows only in the reader's normal mode, without a dialog on top. Presented images and avatars retain their pixels and receive no action underlines. Holding Ctrl or Cmd underlines the other visible interactive targets too.

`esc` with a ring on the screen only clears the ring. Any other key clears the ring and then does its usual job, so `j` still moves the selection. After `⏎` the ring stays on the same target, unless the press changed the selection or opened another mode, such as a dialog or the composer. The ring also clears when its target leaves the screen. A click presses its target without leaving a ring.

In split view the ring crosses both panes. A target in the pane with the keys selects its post as soon as the ring lands on it. A target in the other pane only moves the ring, and `⏎` there first moves the keys to that pane (the feed takes the keys back, the preview takes them), then selects the post and presses the target.

Dialogs keep their own cursor. In a picker, the share dialog (its rows and the checkboxes on the Quote row), the filter panel, the drafts list and Inspect (`.`), `tab` and `shift-tab` move the cursor the way `j` and `k` do. The command line keeps `tab` for completion, and the composer and the search and ask inputs keep their own handling of `tab`. On Settings, `tab` and `shift-tab` move the row cursor and wrap.

### The mouse

`ui.mouse` turns the mouse on or off, and it is on by default. `--mouse on|off`, then `SOCLI_MOUSE`, then `ui.mouse` choose it, and the `Mouse` row in Settings changes it at once and saves it. While it is on, the reader asks the terminal for mouse reports when it starts and turns them off when it leaves, after a panic too. The terminal then reports clicks, drags and the wheel. It reports pointer movement only while a picker, the share dialog, the filter panel, the drafts list or the command list is open, because hovering does nothing anywhere else. socli reads only the left button, and it ignores button releases and drags.

- A click presses the target under it. A click on a post's rows away from any target selects the post, and a click on the post that is already selected opens it, like `⏎`. In split view a click in the pane without the keys moves the keys there first.
- While a picker, the share dialog, the filter panel, Inspect, the drafts list or the command list is open, a click on one of its rows moves the cursor there and chooses that row. A picker picks (the theme picker saves), the share dialog runs the row's action, a checkbox on the Quote row toggles, the filter panel toggles that network, the drafts list loads the draft into the composer, and the command list runs the command. A click outside the box closes it like `esc`, and the theme picker restores the theme it opened on. A click outside the command list leaves the command line open, and clicks in the composer, search, ask, the key help and the image viewer do nothing.
- On Settings, a click on `‹` steps the value back and a click on `›` steps it forward. A click elsewhere on a row moves the cursor there, and a click on the row that already has the cursor acts like `⏎` (the Theme row opens the theme picker, `Accounts` opens the accounts page, and the word `login` or `logout` signs in or out) or `space` (a network row toggles the feed, or signs in when that network is signed out).
- One wheel notch is one step. Notches in one direction that reach socli together over the same pane add up to one step of their count, so a fast scroll moves as far as the same notches one at a time. The wheel moves the cursor of an open dialog (the theme picker previews), the Settings cursor, or the selection in the pane under the pointer, like `↓` and `↑`. Over the pane without the keys, the keys move there first. The key help and a page without posts scroll three rows per notch, and the image viewer shows the next or the previous picture.
- Hovering over a row of a picker, the share dialog, the filter panel, the drafts list or the command list moves the cursor to that row, and the theme picker previews the theme under it. Hovering does nothing in the body or on Settings.

With the mouse on, the terminal passes drags to socli instead of selecting text. To select text, hold Shift while dragging (Option in iTerm2), or set `ui.mouse = off`.

## Sign-in screens

The sign-in procedure uses the terminal width, clamped between 20 and 140 columns. A `socli > login` breadcrumb precedes the So_ wordmark; below 36 columns, the wordmark becomes the text `So_`. The wordmark and the step text share column 2. A dim bracket sits at column 0, with one blank column before the text. Labels, details and authorization links wrap within that column, and the active prompt occupies the final row. X sign-in asks which account to use. Each signed-in Chrome, Brave, Edge, Arc or Chromium profile is one row, and the last row opens x.com so a different account can be approved. The keychain line follows that choice. Passing `--browser` and `--profile` skips the question when they match one session. OAuth sign-in shows its authorization link and callback instructions. The signed-in handle, the next commands, and the reader's Continue action continue the same bracket. The last of those rows closes it. The ask welcome box draws the same So_ mark when the pane is wide enough for it, and the five-row S otherwise.

An empty Search carries a notice in place of posts, which reads `Look up a public account with :user @handle, or :search words.` `ctrl-a` opens the ask screen, a local search over the posts already loaded, with a welcome box that shows the logo, the counts of posts, mentions and likes, and `esc to close`. Direct messages are not in the reader.

Opening `socli` at a terminal with no signed-in account shows a welcome screen with only the `socli > login` breadcrumb. The So_ wordmark sits above the introduction and network choices on the left. From 28 columns, network names line up with the wordmark and the selection mark sits in the two columns before each name. Explore the demo comes first, followed by X, Bluesky, dev/null, Threads and Mastodon. On the right, the selected network has a static character-raster logo. Changing the selection with the keyboard, Tab or pointer launches a damped radial water ripple from the center of the selected logo across the entire window, including the margins and breadcrumb row. Foreground labels and controls remain readable above the wave. Ripples overlap and settle. The logo briefly glitches with shifted bands and scrambled ASCII characters at random intervals of two to eight seconds. The renderer uses the active theme's network accent and dim tokens. Plain mode keeps the artwork and wordmark in ASCII. Below 80 columns or 24 rows, the logo artwork, waves and glitches are hidden, and the welcome screen stops its animation timer. A compact menu retains all six choices down to the supported 24×8 terminal minimum; short windows drop the introduction and large wordmark before dropping any choice. `j`, `k` and the arrow keys follow the visible order; Home selects the demo and End selects Mastodon. Enter on a network signs in, while Enter on Explore the demo opens the unsaved local demo. A signed-in account, the demo, Settings, Search and an open thread leave the welcome screen off. Without a terminal, `socli` still exits with the `no accounts yet` line. The [interactive onboarding study](../design/improvements/onboarding.html) previews the artwork and selection behavior.

The animation research includes [TachyonFX’s cell-targeted terminal effects](https://github.com/ratatui/tachyonfx) and [ascii-wave’s attenuated radial ripples](https://github.com/MarujoEn/ascii-wave). The welcome renderer computes its own frames without adding a dependency.

Provider sign-in screens use one text column. Instructions and authorization URLs wrap above the active prompt, whose label and input share the final row. Typed handles remain visible and secrets display bullets. Authorization links use OSC 8 hyperlinks on supported terminals, so Cmd-click or Ctrl-click opens the complete URL even when it wraps across several rows. Escape, Ctrl-C and Ctrl-D cancel an input prompt. A sign-in opened from the reader remains in the plain CLI procedure after the provider returns: a failure shows the error and selectable Retry and Cancel actions, while success shows Continue, selected by default with Enter. Only Continue or Cancel returns to the reader. The loading loop neither consumes prompt input nor repaints over the procedure. Explicit `socli login <network>` commands print their result and exit without starting the reader or showing these follow-up actions.

Sign-in procedures opened from the reader use its alternate screen throughout the session. Each procedure starts with a cleared screen, and returning to the reader replaces the procedure with the app. Quitting during a prompt restores the shell screen without leaving login steps in its scrollback or clearing earlier shell history. Direct `socli login <network>` commands retain their output in the terminal as usual.

Live startup and successful sign-in select Discover where available, using the provider's For you or Trending feed when those implement discovery. The selected tab is initialized after credentials reload, and Continue fetches the selected feed before displaying it. Threads falls back to Your threads because this integration does not expose a general discovery feed. Explicit feed commands and subsequent tab selections retain their requested feed.

## Spinners

The reader animates two things. The loading glyph cycles `◐ ◓ ◑ ◒` every 120 ms in the rich level, while a request has been running for at least 150 ms. The picture frames in the rich level keep a dim `image` label rather than a spinner. Plain and minimal levels show text only for that glyph. The signed-out home, under Sign-in screens, repaints about every 33 ms while its artwork is visible. Compact welcome screens use the reader’s idle refresh interval rather than the animation timer.

## Icons

socli draws icons from one of two sets. The Nerd Font set uses Material Design glyphs from the Nerd Fonts private use area, which a font patched with Nerd Fonts must provide. The Unicode set needs no special font. The table lists each icon with its Nerd Font name and codepoint and its Unicode replacement.

| Icon | Nerd Font glyph | Unicode |
|---|---|---|
| reply | `nf-md-comment_outline` U+F0182 | `↩` U+21A9 |
| repost | `nf-md-repeat` U+F0456 | `⇄` U+21C4 |
| like | `nf-md-heart_outline` U+F02D5 | `♡` U+2661 |
| liked | `nf-md-heart` U+F02D1 | `♥` U+2665 |
| bookmark | `nf-md-bookmark_outline` U+F00C3 | `☆` U+2606 |
| bookmarked | `nf-md-bookmark` U+F00C0 | `★` U+2605 |
| views | `nf-md-chart_bar` U+F0128 | `◉` U+25C9 |
| more | `nf-md-dots_horizontal` U+F01D8 | `⋯` U+22EF |
| verified | `nf-md-check_decagram` U+F0791 | `✓` U+2713 |
| bell | `nf-md-bell_outline` U+F009C | `⚑` U+2691 |
| link | `nf-md-link_variant` U+F0339 | `↗` U+2197 |
| image | `nf-md-image_outline` U+F0976 | `▣` U+25A3 |
| check | `nf-md-check` U+F012C | `✓` U+2713 |
| cross | `nf-md-close` U+F0156 | `✗` U+2717 |
| at | `nf-md-at` U+F0065 | `@` |
| gauge, 8 steps | `nf-md-circle_slice_1` to `nf-md-circle_slice_8`, U+F0A9E to U+F0AA5 | `◔ ◔ ◑ ◑ ◕ ◕ ● ●` (U+25D4, U+25D1, U+25D5, U+25CF) |

The repost glyph uses `nf-md-repeat`, whose ink height matches the reply and bookmark glyphs in the bundled Symbols Nerd Font Mono. The earlier `nf-md-repeat_variant` had about 61% of that height. The [action comparison](../design/action-spacing.html) shows the correction and the shared spacing.

The icon set is chosen from, in order: `--no-icons` (Unicode), `--icons auto|nerd|unicode`, `SOCLI_ICONS`, a non-empty `NO_ICONS` (Unicode), `ui.icons`, and finally detection. Detection picks Nerd Font glyphs only when `TERM_PROGRAM` is `ghostty` or `WezTerm` or `TERM` is `xterm-ghostty`. VS Code, Cursor and unknown terminals get Unicode. Without a terminal on stdout, or in agent mode, socli ignores `TERM` and `TERM_PROGRAM`, so detection chooses Unicode. The recommended font for the Nerd set is GeistMono Nerd Font.

## Hyperlinks

The reader and `--print` mark names, handles, times, mentions, URLs, card text and the share dialog's values as OSC 8 hyperlinks when the terminal supports them, so a `⌘-click` or `ctrl-click` opens them. A name or handle links to the author's profile page, a time links to the post's permalink, and a URL links to itself. The profile pages are `https://x.com/<handle>`, `https://bsky.app/profile/<handle>`, `https://dev-null.sh/@<handle>` and `https://www.threads.com/@<handle>`. Only `http` and `https` URLs made of printable ASCII (0x21 to 0x7e) become links.

`SOCLI_HYPERLINKS=1` or `0` overrides detection. Otherwise links are off when stdout is not a terminal, in agent mode, when `TERM=dumb`, and inside tmux (`TMUX`) or screen (`STY`). They are on for `TERM_PROGRAM` of `vscode`, `iTerm.app`, `WezTerm`, `ghostty`, `Hyper`, `Tabby` or `rio`, for a `TERM` containing `kitty`, `ghostty`, `wezterm`, `foot` or `alacritty`, for `VTE_VERSION` 5000 or higher, and when `WT_SESSION` or `KONSOLE_VERSION` is set.

## Images

Pictures reach the terminal through the `ratatui-image` picker in the reader and through `viuer` for `--print` and `socli view`. `--images[=auto|kitty|iterm|sixel|blocks|off]`, `SOCLI_IMAGE` and `ui.images` choose the mode, in that order, and `auto` is the default (`--images` alone means `auto`). The Settings row takes the same six values.

- `auto` asks the terminal what it supports and uses the best protocol: kitty, iTerm2 or sixel. A terminal with none of them gets half blocks. Inside tmux auto skips the query and draws half blocks, because tmux needs `allow-passthrough` and the query would turn it on without asking. In VS Code and Cursor auto draws half blocks unless the terminal answers the sixel query, which needs `terminal.integrated.enableImages`. Apple Terminal (`TERM_PROGRAM=Apple_Terminal`) is not asked: it prints the kitty probe, and a kitty or iTerm image, as text, so socli draws half blocks there.
- `kitty`, `iterm` and `sixel` force one protocol, except in Apple Terminal, which still draws half blocks. `blocks` forces half blocks. `off` draws no pictures and drops the `v view` hints. In `sixel` mode the reader's inline images use sixel, while the full-screen viewer and `--print` still draw half blocks, because `viuer` is built without sixel.

Images from posts, profiles and link pages load only from public addresses. socli fetches them with the client that reads link pages, which refuses loopback, private, link-local and other non-public addresses, checks every redirect hop the same way and stops at 8 MiB. A local file or a `file://` URL opens only when the user types it as the target of `socli view <path>`. A typed `http` or `https` target may point at any host, with a 20 MiB cap and at most five redirects. One decoder reads every picture, in the reader, in `--print --images` and in `socli view`. It refuses an image wider or taller than 8192 pixels or one that needs more than 64 MiB to decode, and a truncated or corrupt file fails with `couldn't decode the image`.

Only a graphics protocol draws profile pictures. Half blocks draw inline pictures and the `v` viewer, but the rich level keeps the letter boxes for avatars. `v` opens the viewer for the selected post's images. `--print` never draws link previews or inline pictures, it only lists the images as `▣ <kind> · <author>` lines at the end, and the demo's pictures are generated locally (`demo://` URLs, so the JSON of a demo post shows `demo://avatar/<user>`).

## Website

The homepage introduces the unified TUI feed, direct CLI commands, publishing across networks, threads and profiles, terminal appearance and images, and JSON output. Each feature links to its documentation. Setup details and option lists belong in the guides and references.

WebAssembly appears as a regular section in the feature grid, with the same heading and paragraph styling as the other features. Its present-tense copy jokes about bringing a terminal UI back to the web, links to Woz's website and opens `website/docs/web.md` through its heading.

The `soclib` embed uses this reader's cell renderer, palettes, Geist Mono, and Symbols Nerd Font subset inside a shadow root. It adds no website header or navigation. Each instance fills its host container, fits its own grid, and keeps its styles and keyboard focus separate from other instances. Cell spans, drawn lines, and image layers share `packages/web/renderer.css` with the website. The embed's light and dark surfaces follow the reader palettes, and a visible inset focus ring marks the keyboard target.

`website/docs/keys.md` owns the keyboard reference. The homepage includes its `reader-keys` region, while the documentation page also covers composer, dialog and Settings controls. The documentation navigation and quick-start index link to Keys and CLI commands. The CLI reference distinguishes shell commands from the interactive command palette and scrollback prompt.

## Known differences from the mockups

The mockups in `design/` lead the code in a few places. These differences can be checked in both the code and `design/screenshots/dark/02-feed.png` and its source `design/src/feedparts.js`.

- The chips and posts in the `pick-network` and `pick-theme` mockups show four networks and leave out Mastodon (`NETS` in `design/src/feedparts.js`). The code shows every signed-in network.
- The mockup header shows `4 accounts` and a bell with `3`. The code draws `Accounts` for the merged view, including the demo, or `@handle` for one network, and the bell count. Both labels are pressable in the reader.
- The mockup sub bar ends with `↑ 12 new posts` in blue. The code draws nothing at the right of the sub bar and has no new-post counter.
- The mockup draws a poll result bar in the notifications screen with the `faint` token. The code has no `faint` token and no poll bar.
