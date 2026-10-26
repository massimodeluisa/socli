# Reddit

SoCLI does not support Reddit. The owner decided to skip it for now, and no Reddit code exists in the repository: there is no provider under `src/net/`, no `[reddit]` section in `config.toml`, and no `reddit` value for `--net`. This page records why, what an integration would need, and which options were on the table, so the question can be reopened without redoing the research.

Part of the [integration overview](../README.md).

## Status

Not integrated, by the owner's decision: "Skip Reddit for now".

## Why

Since November 2025 Reddit requires approval for every new Data API app, and approval is mostly refused for personal projects. Apps created before that date keep working. Reddit blocks unauthenticated JSON requests with a 403 response. RSS feeds still work for the public listing of a subreddit and for the private front-page feed link a signed-in user can copy from `reddit.com/prefs/feeds`.

Reddit's own pages that state the approval rule block scripted requests, so they are cited here as the source of the rule and were not fetched when this page was written (see the sources below).

## What an integration would need

Two routes exist, and they differ in what SoCLI could offer:

- An approved Data API app that uses OAuth with read and write scopes. This is the only route that supports posting, replying and a personal home feed, and it depends on Reddit approving the app.
- A read-only mode over RSS. It needs no approval and no sign-in for public subreddits, and it can follow the private front-page feed through the link from `reddit.com/prefs/feeds`. It cannot write.

## Options offered to the owner

1. A read-only Reddit through RSS.
2. An older Data API app, for a developer who already created one before the approval rule.
3. Skip Reddit.

The owner chose the third option. Adding Reddit later would follow the steps in [how a new network plugs in](../README.md#adding-a-network): a provider under `src/net/reddit/` and the five documentation pages under `docs/integration/reddit/`, which would replace this page.

## Sources

Both pages block scripted requests. They are cited as the sources of the November 2025 approval rule.

- [Responsible Builder Policy](https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy)
- [Reddit API documentation](https://www.reddit.com/dev/api)
