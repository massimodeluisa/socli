# Rust source review for 0.1.4

The October 2026 review concentrated on failures that could lose a draft, report an action as successful without confirmation, replace a newer login, or leave the reader unable to recover after a network outage. The fixes ship together in 0.1.4. This record explains the review's coverage and its limits.

## Source coverage

The review divided the Rust source into four areas and checked the resulting changes together. Source searches and module-level tracing covered every Rust module; authentication, transport, pagination, writes, persistence and background work received the deepest inspection. This was a source review with regression tests, rather than a formal proof that every execution path is correct.

| Area | Reviewed responsibilities |
| --- | --- |
| X and shared transport | OAuth and browser sessions, cookie import, PKCE, official API requests, private GraphQL, transaction headers, provider errors and bounded response reads. |
| Other providers | Bluesky sessions and XRPC, rich-text facets, dev/null device and SSH signatures, Mastodon authentication and HTML mapping, Threads authentication and preparation polling, provider capabilities and pagination. |
| Reader | State transitions, merged feeds, action counters, composer persistence, terminal and browser input, rendering and control-text cleaning, image loading, caches, worker shutdown and terminal recovery. |
| Application and distribution | Command parsing and dispatch, configuration migration and atomic transactions, drafts, output and exit codes, clocks and models, native packaging, npm packages, release metadata and GitHub workflows. |

The module boundaries remain the same: providers translate network protocols, the shared HTTP layer classifies transport failures, state applies confirmed changes, and rendering consumes that state. `State::write_post_checked` preserves typed errors until draft persistence has recorded whether delivery is uncertain. The existing string-returning entry point remains available. The TUI's test suite now lives in `src/tui/tests.rs` beside the production module.

The crate is allocation-conscious: posts, decoded images and response documents use owned storage. It does not claim heapless execution. API response reads are capped at 8 MiB, typed local images at 20 MiB, and the native preview loader limits its pending queue and remembered entries. Loaded feed history and link-preview bookkeeping grow with the session.

## Failure behavior

A write is successful only after its provider confirms the result. An explicit rejection preserves the previous local action state. A transport failure or incomplete acknowledgement can leave the remote result unknown; those errors retain a warning in the durable draft. Autosave, a dry run and discarded edits preserve that warning. Checking the remote account remains necessary before retrying an uncertain post.

A failed refresh leaves the previous credentials available. Saving refreshed credentials checks that the stored session still matches the one being refreshed, so a delayed response cannot restore a logged-out account or replace a different login. Environment overrides cannot combine a replacement token or server with another stored session's refresh token, cookies or account identity. Independent processes can still race during the remote token exchange; the local compare-before-save guard does not serialize that exchange.

Partial feed refreshes retain cached posts and cursors for networks that failed. Repeated next-page cursors stop that provider with a notice, while successful pages remain visible. Image fetches retry transient errors with bounded delays while the image is still requested by the current frame. Invalid image data remains a decoding error.

## Verification

The regression suite uses fixtures, temporary configuration directories and loopback HTTP servers. It covers expired sessions, rate limits during refresh, changed credentials, rejected and unconfirmed mutations, partial outages, cursor boundaries, counter saturation, draft write failures, terminal control text and worker shutdown. It does not publish posts, likes, follows or other actions to real accounts.

The release verification runs the workspace tests, native and WebAssembly builds, formatting checks, dependency policy checks, workflow linting, package tests and worker tests. Typecheck commands are excluded from the local review at the owner's request. Release CI separately runs the repository's configured Rust checks and builds the supported distribution targets before publishing.

The review follows the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) and [Rust Style Guide](https://doc.rust-lang.org/stable/style-guide/), with the repository's compatibility, module and output conventions taking precedence for application-specific choices.

## Remaining boundaries

`Provider::thread` has no continuation-cursor argument. Threads conversation reads retain their five-page bound, dev/null reads one reply page, and Bluesky uses bounded ancestor and reply-depth requests. Feed pagination improvements do not establish arbitrary completeness for large conversations.

X's private GraphQL operations can change without a stable API contract. Existing OAuth sessions need another sign-in to grant the newly requested like and follow permissions. Threads remains unavailable outside demo mode pending Meta approval.

Tests verify the source against controlled responses. They do not establish current behavior on every live Mastodon server, real account permissions, platform delivery guarantees or the behavior of a future upstream API change. The [changelog](../CHANGELOG.md) lists the shipped changes, and [release instructions](RELEASING.md) describe the checks and publishing destinations.
