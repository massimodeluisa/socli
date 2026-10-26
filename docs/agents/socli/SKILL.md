---
name: socli
description: Read and write X, Bluesky, dev/null, Threads and Mastodon from the terminal with the SoCLI CLI. Use when the user asks to read a feed, a thread, mentions or search results on one of those networks, or to post, reply, quote, like, repost, bookmark, follow or delete there. Covers JSON output, scoped post ids, dry runs and the confirmation rule for public writes.
---

# SoCLI

SoCLI is a terminal client for X, Bluesky, dev/null, Threads and Mastodon. When an agent runs `socli`, output is JSON, nothing prompts and no key is read. Run `socli help <command>` for the flags of one command, or `socli help --json` for the full table.

## Rules

1. Read before you write. Every read prints a JSON array of posts, and every post has a scoped `id` such as `bluesky:at://…`, `x:1849213377`, `devnull:p2` or `mastodon:115000000000000001`. Pass that string to the next command. Do not guess ids.
2. Run each write with `--dry-run` first, show the person the text and the networks, and send it only after they confirm. `post`, `reply`, `quote`, `delete` and `follow` are public or destructive.
3. Never pass `--yes` to `delete` without the person's explicit confirmation of that exact post.
4. Take links from the `url` or `permalink` field of a post. Do not call `socli share`: it prints the same link and adds nothing for an agent.
5. Keep tokens and app passwords out of command lines and out of anything you print.

## Reading

```sh
socli feed --net bluesky              # the home timeline of one network
socli feed saved --net bluesky        # bookmarks, or starred commits on dev/null (also: following, latest, trending, discover, mine, notifications)
socli thread bluesky:at://…           # a post and its replies
socli search "rust tui" --net bluesky
socli user @mina --net x              # one account's posts
socli notifications                   # mentions
socli whoami                          # which networks are signed in
socli auth status                     # configuration, secrets redacted
```

Without `--net`, SoCLI reads every signed-in network. `--net` takes `x`, `bluesky`, `devnull`, `threads`, `mastodon`, a comma list or `all`. `--count <n>` sets the posts asked for per network (15 by default). On X each post read is billed, so do not page without a reason.

Useful post fields: `id`, `network`, `url`, `text`, `created_at`, `author.username`, `in_reply_to`, `conversation_id`, `metrics`, `liked`, `reposted`, `bookmarked`, `links`, `media`, `quoted`.

## Writing

```sh
socli post "hello" --net x,bluesky --dry-run
socli post - --net bluesky < message.txt       # text from standard input
socli reply bluesky:at://… "agreed" --dry-run  # replies go to the parent's network
socli quote x:1849213377 "this" --dry-run
socli like x:1849213377                        # also: unlike, repost, unrepost, bookmark, unbookmark
socli follow @mina --net x                     # also: unfollow
socli delete x:1849213377 --yes                # after confirmation only
```

A write prints an array with one object per network: `network`, `ok`, and `id`, `url` or `error`. A dry run adds `length` and `limit`, so you can see a post that is too long (limits: X 280, Bluesky 300, dev/null 4000, Threads 500, Mastodon 500 by default, or the limit its server reports). Exit code 1 means at least one network failed, and the objects say which.

`<b>` and `<i>` in post text become Unicode bold and italic letters. Pass `--raw` to send the text exactly as given.

## Errors and exit codes

A failure prints one JSON line on stderr and nothing on stdout, for example `{"error":"which network? add --net x|bluesky|devnull|threads|mastodon, or use a full id like bluesky:at://…"}`. The message says what to run next, so show it to the person or act on it.

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | A network or target failed (not signed in, rate limit such as `<Network> rate limit reached; try again in N minutes`, unsupported feature, missing post) |
| 2 | A usage error, or a step that needs a person at a terminal |

Some networks lack features: Threads has no likes and no discover feed, and reposts there cannot be undone. The error says so, and retrying does not help.

## Signing in

You can sign in to Bluesky, Threads and Mastodon without a person:

```sh
printf '%s\n' "$APP_PASSWORD" | socli login bluesky --handle you.bsky.social --password-stdin
socli login threads --token "$THREADS_ACCESS_TOKEN"
socli login mastodon --token "$MASTODON_ACCESS_TOKEN" --server mastodon.social
```

X and dev/null sign-ins need a browser, and so does `socli login mastodon` without a token. In agent mode `socli login x`, `socli login devnull` and a token-less `socli login mastodon` exit 2 with a message such as `signing in to X needs a person at a browser`. Ask the person to run the command in a terminal, then continue with the stored session. Never run `socli logout` unless the person asks for it.

## More

The full guide, with every JSON shape, is `docs/agents.md` in the SoCLI repository. `--demo` runs against an offline feed and sends nothing, which is the right way to test a script.
