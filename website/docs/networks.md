---
title: Networks
description: What SoCLI can read and write on X, Bluesky, dev/null, Threads and Mastodon, and what each network leaves out.
---

# Networks

SoCLI talks to five networks, and each one exposes a different part of itself to third-party apps. The table below lists what SoCLI does on each network once you are signed in with the strongest access that network offers. A cell that says no is refused before SoCLI sends a request, with a message that names the network and the missing feature, for example `Threads has no likes in its API`. The sections after the table cover what works with less access, the limits that change how SoCLI behaves, and what is not in SoCLI yet.

<div class="matrix-wrap">

| | X | Bluesky | dev/null | Threads | Mastodon |
| --- | --- | --- | --- | --- | --- |
| Sign-in | The browser session already signed in, or OAuth with your own developer app. The page approves SoCLI's public app | App password | Device key you approve in the browser | Approve SoCLI's Threads app in the browser. A token you already have still works | Your server, default mastodon.social, or a pasted token |
| Feeds | For you, Following, Bookmarks | Following, Discover, Bookmarks | Following, Latest, Trending, Starred | Your threads | Home, Trending, Bookmarks |
| Merged Home | Following | Following | Following | Your threads | Home |
| Merged Discover | For you | Discover | Trending | none | Trending |
| Merged Saved | Bookmarks | Bookmarks | Starred | none | Bookmarks |
| Mentions | yes | yes | yes | yes | yes, mentions only |
| Search | yes | yes | no | yes | yes |
| Threads | yes | yes | yes | yes | yes |
| Profiles | yes | yes | yes | your own only | yes |
| Post and reply | yes | yes | yes | yes | yes |
| Quote | yes on a browser session; your text plus the link on the official API | yes | yes | yes | yes |
| Like | yes | yes | star | no | favorite |
| Repost | yes, with undo | yes, with undo | no | yes, no undo | boost, with undo |
| Bookmark | yes | yes | no | no | yes |
| Follow | yes | yes | yes | no | yes |
| Delete | yes | your own posts | yes | yes | yes |
| Counts | yes | yes | replies and stars | no | yes |
| Views | yes | no | no | no | no |
| Length | 280 | 300 graphemes | 4000 characters | 500 characters | your server's limit, 500 by default |

</div>

The merged rows describe the reader with more than one network signed in. Its Home tab takes each network's primary feed, Discover takes the network's discovery feed, and Saved takes bookmarks, plus the commits you starred on dev/null. `socli feed` reaches the same feeds by name (`following`, `latest`, `trending`, `discover`, `saved`, `mine` or `notifications`), on the networks that have that feed.

## X

`socli login x` imports the X session already signed in inside Chrome, Brave, Edge, Arc or Chromium. For you and Following on that session are the website's Home and Latest timelines, and a quote is a real quote. `socli login x --oauth --client-id <id>` uses the official API and needs a developer app at console.x.com. On that path the For you tab is not the ranked feed you see in the X app, because X does not offer that feed to third-party apps. SoCLI runs a recent public search in its place, set by `latest_query` in the `[x]` section of the config (default `lang:en -is:retweet`). X has no trending feed in SoCLI.

An app bearer token, stored with `socli auth x bearer`, is enough for search, threads, profiles, counts and views. Feeds, mentions and every write need `socli login x`.

The official API bills reads per post returned. A shell read asks for 15 posts (change it with `--count`) and prints that page. The reader asks each signed-in network for at least 20 and starts one next page when the selection reaches the 10th post of the current page (`m` loads one page too, and the keys keep moving). It does not keep paging through the whole timeline. The split view asks for that conversation and shows its replies on the right. Quoting through that API needs an Enterprise plan, so a quote there is your text followed by the link of the quoted post. X moved like and follow writes off its self-serve tiers, and replies from self-serve apps are often refused unless the author has mentioned or quoted you. When X answers 403 to one of those calls, SoCLI's error names that cause. A browser session does not use those billed endpoints.

## Bluesky

`socli login bluesky` asks for your handle and an app password created at bsky.app/settings/app-passwords. Without a session, SoCLI reads threads, profiles and counts from Bluesky's public AppView. Search is closed to signed-out apps on that AppView, so it needs the app password too, like every feed and every write.

## dev/null

dev/null has three levels of access. Public access opens threads and profiles. A GitHub SSH key, chosen with `socli auth devnull ssh`, adds posts and replies. The device key from `socli login devnull`, once you approve it in the browser, opens the four feeds, mentions, stars, quotes, follows and deletes, and the replies inside a thread.

The dev/null API has no search, reposts or bookmarks. Its private list is your starred commits, which SoCLI shows as the Starred tab and in the merged Saved tab. Listing notifications marks them as read on the server, and the feeds return 20 commits per page whatever `--count` asks for.

## Threads

`socli login threads`, a pasted token, and `socli auth threads oauth` answer Waiting for Meta approval and do not call Meta. Outside `--demo`, a live session leaves Threads out of the feed. The Threads API has no home timeline, so SoCLI's only feed there is your own threads. It also has no likes, bookmarks or follows, no way to undo a repost, and no profile endpoint for other accounts. SoCLI does not request reply or like counts from Threads.

A Threads quote is native when the quoted post is on Threads, and your text plus the link otherwise. Until Meta approves the keyword search permission for your app, search covers only your own posts, and until it approves publishing for advanced access, the app can post only for its testers. The API cannot turn a post link into an id, so `socli thread` on a Threads link asks for the id from `socli --json` instead.

## Mastodon

`socli login mastodon` asks for your server (Enter keeps mastodon.social), registers SoCLI as an app there and opens the authorization page. `socli login mastodon --token <token>` stores a token you created in Preferences > Development with the scopes `read write`, and needs no browser.

Signed out, SoCLI opens threads and profiles only, because servers such as mastodon.social keep their live feeds closed to signed-out readers. A like is a favorite and a repost is a boost, and the reader uses those words on Mastodon posts. Quotes are native on servers that run Mastodon 4.5 or newer when the quoted author allows them, and an older server gets your text plus the link.

The length limit is the one your server reports at sign-in, 500 when it does not answer. SoCLI counts graphemes, counts every link as 23 characters, and counts a mention such as `@theo@fosstodon.org` as `@theo`, the way Mastodon does. Status search depends on a search backend on the server, so some servers return no posts.

## Not in SoCLI yet

Some features exist on the networks but not in SoCLI yet.

Direct messages sit behind separate APIs on every network that has them. X's direct-message endpoints need their own permissions, Bluesky routes chat through a separate service that an app password reaches only when you allow it, and Mastodon keeps private mentions as conversations. Threads and dev/null have no direct-message API. The reader has no Messages section until SoCLI reads direct messages.

SoCLI opens profiles with `u` on a post or `:user @handle`, subject to each network's limits (Threads exposes your own profile only). It does not edit your profile. Bluesky and Mastodon have APIs for profile editing, while X's v2 API has no profile update, the Threads profile endpoints are read-only, and dev/null mirrors your profile from GitHub.

Posts carry text only on every network. SoCLI shows images and polls and opens videos in the browser, but it does not attach any of them, and it cannot vote in a Mastodon poll. X lists, Bluesky feeds other than Following and Discover, and Mastodon lists have no code. Reddit is not integrated.

## Colors

Each network has a chip color: X uses the body color, Bluesky sky, dev/null amber, Threads violet and Mastodon indigo. `socli config set bluesky.color teal` changes one, and the value is a palette name or `#rrggbb`. See [Configuration](config.html).
