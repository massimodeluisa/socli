---
title: Keys
description: Keyboard shortcuts for the socli terminal reader, composer, command palette, dialogs and settings.
---

# Keys

## Reader shortcuts

<!-- #region reader-keys -->
These shortcuts apply to the full-screen terminal reader. Press `?` to open its key reference. Movement keys scroll the reference when it exceeds the terminal height; another key closes it. Post actions depend on what each network supports.

<div class="key-groups">

<div class="key-group">

### Move

| Key | Action |
| --- | --- |
| `j` `k` `↓` `↑` | Move to the next or previous post. A taller post is crossed first. |
| `space` `PgDn` `ctrl-f` | Move a page down. A taller post is crossed first. |
| `PgUp` `ctrl-b` | Move a page up. A taller post is crossed first. |
| `ctrl-d` `ctrl-u` | Move half a page down or up. A taller post is crossed first. |
| `g` `Home` / `G` `End` | Select the first or last post. |
| `⏎` `→` | Open the selected thread or folded replies. |
| `⌫` `←` `esc` | Go back. With a focus ring, `esc` clears the ring first. |

</div>

<div class="key-group">

### Act

| Key | Action |
| --- | --- |
| `l` | Like; press again to undo (favorite on Mastodon, star on dev/null). |
| `t` | Repost; press again to undo where supported. |
| `b` | Bookmark; press again to remove. |
| `r` | Reply to the selected post. |
| `n` | Compose a new post. |
| `o` | Open the selected post's link, or the current profile, in a browser. |
| `v` | View the selected post's images. |
| `u` | Open the author's profile. |
| `s` | Open the share dialog to copy a link, repost or quote. |
| `m` | Load the next page. |
| `ctrl-r` `cmd-r` | Reload the page on screen. `r` still replies. |
| `.` | Inspect the selected post as JSON. |

</div>

<div class="key-group">

### Go

| Key | Action |
| --- | --- |
| `1` to `9` | Open a feed tab. |
| `{` `}` | Open the previous or next feed tab. |
| `[` `]` | Open the previous or next section. |
| `tab` `shift-tab` | Focus the next or previous control; `⏎` activates it. |
| `F6` `shift-F6` | Move focus to the next or previous zone. |
| `:` `ctrl-k` | Open the command palette. |
| `f` `/` | Open search. |
| `ctrl-a` | Open Ask to search posts already loaded. |

</div>

<div class="key-group">

### View

| Key | Action |
| --- | --- |
| `d` | Cycle and save the detail level. |
| `D` | Pick and save a detail level. |
| `T` | Preview a theme, then select it to save. |
| Hold `ctrl` or `cmd` | Reveal shortcuts and underline actions where the terminal reports modifier keys. |
| `x` | Show or hide replies in the current view. |
| `a` | Cycle through signed-in networks, then all networks. |
| `A` | Open the network picker. |
| `F` | Open the network filter. |
| `0` | Show all signed-in networks. |
| `<` `>` | Show the previous or next network. |
| `,` | Open Settings. |
| `⌘-click` `ctrl-click` | Open names and links in the browser in terminals that support hyperlinks. |

</div>

<div class="key-group">

### Other

| Key | Action |
| --- | --- |
| `?` | Open the key reference. |
| `q` | Quit the reader. Dialog shortcuts depend on context. |
| `ctrl-c` | Quit; in the composer, edited text opens the save-draft question. |

</div>

</div>
<!-- #endregion reader-keys -->

## Composer

Press `n` to compose a post or `r` to reply. With the default `ui.enter = post`, `⏎` sends the text and `ctrl-j` inserts a newline. `shift-enter` also inserts a newline in terminals that report the kitty keyboard protocol.

With `ui.enter = newline`, `⏎` inserts a newline and `ctrl-s` sends. `ctrl-enter` can also send in this mode when the terminal reports it.

| Key | Action |
| --- | --- |
| `tab` `shift-tab` | Cycle the destination for a new post with one target. Replies and quotes keep their targets. |
| `ctrl-o` | Open saved drafts. |
| `↑` `↓` | Move between lines. |
| `esc` | Leave; edited text opens the save-draft question. |

In the draft question, `y` saves, `n` discards and `esc` returns to editing. In the drafts list, `j`/`k`, the arrow keys or `tab`/`shift-tab` move, `⏎` loads a draft, `d` deletes it and `esc` or `⌫` closes the list.

Pasted text keeps its newlines and never sends the post. The [Reader](reader.html#composer) page explains how drafts survive failed sends.

## Text editing

The composer, search, Ask and command inputs share these editing keys. The composer moves by whole characters, including accents and emoji.

| Key | Action |
| --- | --- |
| `←` `→` | Move the cursor. |
| `alt-←` `ctrl-←` `alt-b` / `alt-→` `ctrl-→` `alt-f` | Move a word left or right. |
| `Home` `ctrl-a` / `End` `ctrl-e` | Move to the start or end of the line. |
| `⌫` `Delete` | Delete before or after the cursor. |
| `alt-⌫` `ctrl-w` | Delete the previous word. |
| `ctrl-u` | Delete to the start of the line, except in Ask, where it scrolls. |
| `ctrl-z` | Undo. |
| `ctrl-y` `ctrl-shift-z` | Redo. |

## Command palette

Open it with `:` or `ctrl-k`. Type a command, use `↑`/`↓` or `ctrl-p`/`ctrl-n` to choose a suggestion, `tab` to complete and `⏎` to run. `esc` closes it; `⌫` also closes an empty input. The [command reference](commands.html#in-reader-commands) lists its syntax, which differs from shell commands.

## Search and Ask

In search, `⏎` runs the query and `esc` closes the input. In Ask, `⏎` submits a question about posts already loaded. Ask matches local text and does not call an AI model. `PgUp`/`PgDn` and `ctrl-u`/`ctrl-d` scroll its results, and `esc` returns to the reader.

## Pickers and network filter

In the detail and network pickers, `j`/`k`, `↓`/`↑` or `tab`/`shift-tab` move the cursor. `⏎`, `space` or a displayed digit chooses an entry. `esc`, `⌫`, `←` or the key that opened the picker closes it. Choosing a different detail level applies and saves it as the default; `s` saves the highlighted level too.

The theme picker uses the same movement keys and previews as you move. `⏎` or `space` selects and saves; `esc`, `⌫` or `←` cancels and restores the original theme.

The network filter (`F`) uses the same movement keys. `space` toggles a network, `a` selects all signed-in networks and `⏎` applies and saves. At least one network must remain selected. `esc` or `⌫` cancels. Settings changes in demo mode apply only to the session.

## Share dialog

Press `s` on a post or profile. `j`/`k`, `↓`/`↑` or `tab`/`shift-tab` select an action, and `⏎` runs it. `s` copies the post link (the profile link when sharing a profile), `p` copies the post author's profile link, `t` reposts or undoes a repost where supported, and `q` opens a quote composer. `esc` or `⌫` closes the dialog.

When a quote has network checkboxes, `←`/`→`, `h`/`l` or `tab`/`shift-tab` move between them, and `space` toggles the highlighted destination. `⏎` opens the composer with the checked destinations.

## Image viewer and JSON inspector

In the image viewer, `←`/`→` or `h`/`l` select the previous or next image, `o` opens it in a browser and `s` opens the post's share dialog. `esc`, `⌫`, `v` or `q` closes the viewer.

In the JSON inspector (`.`), movement and paging keys select a row, `tab`/`shift-tab` move one row and `⏎` expands or collapses the selected value. `esc`, `⌫` or `←` closes it.

## Settings and accounts

In Settings, `j`/`k` or `↓`/`↑` move between rows, and `tab`/`shift-tab` wrap through them. `h`/`l` or `←`/`→` change a value. On a network row, `space` toggles its inclusion in the feed, or starts sign-in if it is signed out. `⏎` opens Accounts, opens the theme picker or signs a network in or out, depending on the selected row. `esc` or `⌫` goes back.

On the Accounts page, `j`/`k` or `↓`/`↑` move, `⏎` or `space` signs the selected network in or out, and `esc` returns to Settings. See [Configuration](config.html) for saved values and [Sign in](login.html) for account setup.
