# Plan: direct messages

socli reads and writes public posts. Direct messages run on separate, private APIs on every network that has them, and until now the reader had a Messages section that only said so: it showed "Direct messages are separate APIs on every network and are not in this reader." That section is gone, and `:messages` and `:dm` no longer open it. This plan records what each network offers for direct messages, the order in which socli adds them, how they appear in the reader and on the command line, and what has to work before a Messages section comes back.

The facts below were read on 2026-10-21 from each network's own documentation, cited next to each one.

## What each network offers

| Network | Read conversations | Send | Sign-in change | Message length |
| --- | --- | --- | --- | --- |
| Mastodon | `GET /api/v1/conversations` | a status with `direct` visibility | none | the server's post limit |
| Bluesky | `chat.bsky.convo.listConvos`, `getMessages` | `chat.bsky.convo.sendMessage` | a new app password with direct message access | 1000 graphemes |
| X | `GET /2/dm_events` and the per-conversation variants | `POST /2/dm_conversations/...` | sign in again with `dm.read` and `dm.write` | not checked yet |
| Threads | none documented | none documented | not applicable | not applicable |
| dev/null | none | none | not applicable | not applicable |

### Mastodon

Mastodon has no separate message store. A direct message is an ordinary status posted with `visibility` set to `direct`, which the API documentation also calls a private mention, and only the accounts it mentions can see it ([statuses](https://docs.joinmastodon.org/methods/statuses/)). The conversations API groups those statuses into threads: `GET /api/v1/conversations` lists them, `POST /api/v1/conversations/:id/read` marks one read, and `DELETE /api/v1/conversations/:id` removes one from your list ([conversations](https://docs.joinmastodon.org/methods/conversations/)). All three exist since Mastodon 2.6.0. Listing needs `read:statuses`, and the other two need `write:conversations`.

socli already signs in to Mastodon with the scopes `read write` (see `docs/integration/mastodon/auth.md`), which include both, so no one has to sign in again. A reply is a direct status with `in_reply_to_id` set, which socli already sends for public replies, and its length is the server's post limit that the composer counts today. Mastodon sets the quote policy of a direct status to `nobody`, so no one can quote it.

### Bluesky

Bluesky keeps direct messages on a separate chat service. A client sends the chat calls to the user's own PDS with the header `atproto-proxy: did:web:api.bsky.chat#bsky_chat`, and the PDS forwards them; that service DID is listed in the [API directory](https://docs.bsky.app/docs/api-directory). The lexicons in `@atproto/api` define the calls socli needs:

| Call | Use |
| --- | --- |
| `chat.bsky.convo.listConvos` | the conversation list, up to 100 per page with a cursor, filterable by read state and status |
| `chat.bsky.convo.getConvoForMembers` | get or create the one-to-one conversation with an account |
| `chat.bsky.convo.getMessages` | the messages of one conversation |
| `chat.bsky.convo.sendMessage` | send one message: text up to 1000 graphemes and 10000 bytes, with an optional quoted post |
| `chat.bsky.convo.updateRead` | mark a conversation read, optionally up to one message |
| `chat.bsky.convo.getLog` | the events after a cursor (new, deleted and read messages, accepted and left conversations), for refreshing without reloading everything |

A conversation is either a `request` or `accepted`, and it can be locked, in which case `sendMessage` fails with `ConvoLocked`. Each account's `chat.bsky.actor.declaration` record says who may start a conversation with it (`all`, `following` or `none`), so a first message to someone can be refused.

The sign-in is the hard part. An app password reaches the chat service only when it was created with "Allow access to your direct messages" ticked, in the Bluesky app's app password dialog. The Python atproto client documents that without it every chat call fails with `Bad token scope` ([guide](https://github.com/MarshalX/atproto/blob/main/docs/source/guides/direct-messages.md)). Everyone who signed in to socli with an older app password has to create a new one with that box ticked and run `socli login bluesky` again, and socli has to recognize the error and say exactly that.

### X

The X API v2 has direct message endpoints for reading and for writing ([lookup](https://docs.x.com/x-api/direct-messages/lookup/introduction), [manage](https://docs.x.com/x-api/direct-messages/manage/introduction)):

| Endpoint | Use |
| --- | --- |
| `GET /2/dm_events` | every recent message event of the signed-in user |
| `GET /2/dm_conversations/with/:participant_id/dm_events` | the one-to-one conversation with an account |
| `GET /2/dm_conversations/:dm_conversation_id/dm_events` | one conversation by id |
| `POST /2/dm_conversations/with/:participant_id/messages` | send to an account |
| `POST /2/dm_conversations/:dm_conversation_id/messages` | send into an existing conversation |
| `POST /2/dm_conversations` | start a group conversation |
| `DELETE /2/dm_events/:event_id` | delete a message |

Reading returns events from the last 30 days only. It needs `dm.read` together with `tweet.read` and `users.read`, and writing needs `dm.write` on top. App-only authentication is refused, since every message is private. A message carries text, attachments or both, with at most one media item. The v2 reference lists no endpoint that marks a conversation read, so X conversations would show no unread state.

socli's default X scopes are `tweet.read tweet.write users.read bookmark.read bookmark.write offline.access` (see `docs/integration/x/auth.md`). Direct messages need the default list to gain `dm.read dm.write`, and every existing X account has to sign in again for its token to carry them. X bills API use per request under pay-per-use, so before this ships the price of a direct message read has to be checked in the developer console and written into the X integration page, the way the timeline cost is today.

### Threads and dev/null

The [Threads API reference](https://developers.facebook.com/documentation/threads/reference) lists endpoints for publishing, media, replies, reply conversations, profiles, locations, insights, oEmbed and access tokens, and none for direct messages, so socli can offer none on Threads. The dev/null API routes file (`routes/api.php` in nunomaduro/dev-null.sh, read 2026-10-21) defines 32 routes and none of them concerns messages or conversations.

## Order of work

Mastodon comes first. It needs no new sign-in, its messages are statuses that socli already parses and renders, and the conversations API is three calls. Bluesky follows: it needs the proxy header, a new data model for conversations and messages, and the app password check. X comes last because every user must sign in again, the history stops at 30 days, and each read costs money.

Each network lands as its own change with its own tests and an update to its five pages under `docs/integration/<slug>/`, and the Networks page on the website gains a Messages row in the same change.

## In the reader

The Messages section returns between Notifications and Search, stepped with `[` and `]` like the others, as soon as one network supports it. It lists conversations from every network in scope, newest message first. A row shows the network badge, the other participants, the first line of the latest message, its age and an unread mark where the network reports one. The network filter (`F`, `a`, `A`, `0`) narrows the list the same way it narrows a timeline.

`⏎` opens a conversation as a transcript, oldest message at the top, with your own messages set apart from the others, and opening it marks it read on Mastodon and Bluesky. `r` writes into the open conversation with the composer, whose counter uses the network's limit. `n` in the section starts a new conversation and asks for the recipient first. A network that is signed in but lacks access shows one line that says what to do, such as creating a Bluesky app password with direct message access and signing in again.

The screen anatomy goes into `docs/DESIGN.md` and a mockup into `design/` before any code, as for every other screen.

## On the command line

A `socli dm` command covers the same ground for scripts and agents. `socli dm list` prints the conversations, `socli dm show <id>` prints one conversation, and `socli dm send <@handle or id> "text" --net <network>` sends a message. Conversation ids are scoped like post ids (`<network>:<native id>`), `--json` prints the same shapes the reader uses, and `--dry-run` prints the request without sending it. Sending follows the write rules that `socli post` follows in agent mode, documented in `docs/agents.md`.

## Privacy

Messages stay in memory. socli writes no message to disk, the composer saves no draft of a direct message to `drafts.json`, and `--print` writes a conversation to the scrollback only when you ask for it. The demo gets its own synthetic conversations, and every test fixture is synthetic.

## Before the section returns

The Messages section and `socli dm` ship only when all of these hold for at least one network:

1. Listing, opening, replying to and starting a conversation work against a real account, and the unread state matches the network's own app.
2. A missing permission (an X token without the DM scopes, a Bluesky app password without DM access) produces a message that names the fix.
3. A refused first message (a Bluesky declaration, a locked conversation, an X account that does not accept messages) produces a readable error, and nothing is retried on its own.
4. README, `docs/agents.md`, `docs/DESIGN.md`, the integration pages and the website's Networks page describe only what shipped.

## Questions for later

- Group conversations exist on X and Bluesky, and on Mastodon a direct status can mention several accounts. The first version could stop at one-to-one conversations.
- Bluesky separates message requests from accepted conversations. They could sit in their own tab of the section or at the end of the list.
- Whether X direct messages are worth their cost once the price is known.
