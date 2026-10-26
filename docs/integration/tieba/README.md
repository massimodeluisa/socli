# Baidu Tieba

SoCLI does not support Baidu Tieba. The owner answered "Maybe also (later)" when it came up, so it is a candidate and not a decision, and no Tieba code exists in the repository: there is no provider under `src/net/`, no `[tieba]` section in `config.toml`, and no `tieba` value for `--net`. This page records what the maintainer found on 2026-10-20, so the question can be reopened without redoing the research.

Part of the [integration overview](../README.md).

## Status

Not integrated. The owner's answer was "Maybe also (later)".

## Why

Baidu offers no official API for Tieba. The clients that exist are unofficial and reverse-engineered: aiotieba (Python), libsgh/tieba-api (Java), MediaCrawler and the npm package tieba-api. They sign in with the account's `BDUSS` login cookie, which carries the same access as the account's password. The libsgh/tieba-api project warns that its reply feature can get a whole forum banned. Because the interfaces are reverse-engineered, they can change without notice.

The maintainer read the project pages listed below and did not fetch them again for this page, so the statements above are the maintainer's reading of those pages on 2026-10-20.

## What an integration would need

An integration would be an unofficial client inside SoCLI that signs in with the `BDUSS` cookie. It would have these properties:

- The cookie is a secret with the power of the account's password. SoCLI would have to store it in `config.toml` with the same care as the tokens of the other networks, and the person signing in would hand over full account access instead of a scoped token.
- Baidu offers no official API, so a change on its side can break the client without warning.
- The libsgh/tieba-api project warns that its reply feature can get a whole forum banned, so a write feature would carry that risk.

Adding Tieba later would follow the steps in [how a new network plugs in](../README.md#adding-a-network): a provider under `src/net/tieba/` and the five documentation pages under `docs/integration/tieba/`, which would replace this page.

## Sources

Checked by the maintainer on 2026-10-20 and not fetched while writing this page.

- [aiotieba](https://docsearch.algolia.com/mcp/docs/repo/lumina37/aiotieba)
- [libsgh/tieba-api](https://awesome.ecosyste.ms/projects/github.com%2Flibsgh%2Ftieba-api)
- [MediaCrawler for Tieba](https://instagit.com/NanmiCoder/MediaCrawler/how-to-use-mediacrawler-for-tieba-data/)
- [tieba-api on npm](https://npmjs.com/package/tieba-api)
