# Changelog

Release notes are kept here. Move entries from `[Unreleased]` into a dated version section before tagging a release.

## [Unreleased]

## [0.1.3] - 2026-10-29

This release fixed cases where a failed request could appear successful, a refresh could replace a newer login, or a saved draft could lose the warning that its post may already have reached the network. It also kept cached feed content available during partial outages and added installers for Linux and macOS.

### Sessions and sign-in

- Preserved Bluesky rate limits and server errors when refreshing an expired session, and rejected refreshed sessions with missing tokens.
- Protected saved X, Bluesky and Threads credentials against refreshes that finished after logout or a different login. Kept rotated X credentials available to the running client when saving them failed.
- Resolved environment credentials as complete sessions. Replacing an X token dropped unrelated stored refresh tokens and cookies; changing a Mastodon server stopped inherited credentials from being sent to the new host; replacing Mastodon or Threads tokens cleared stale account metadata.
- Stopped expired X browser cookies from triggering a refresh of unrelated OAuth credentials, and cleared stale account metadata when importing a different token.
- Requested X's `like.write` and `follows.write` permissions. Existing OAuth sessions need another sign-in to grant those permissions.
- Limited the X browser callback wait to three minutes, bounded each connected callback, and accepted requests whose headers arrived in fragments.
- Removed raw token response bodies, credential-bearing URLs and TOML source values from authentication, transport and configuration errors.
- Rejected dev/null sign-in responses without a username and preserved ancestor authorization and server failures instead of treating them as missing posts.
- Returned the one-hour Threads token when the 60-day exchange failed, and included Meta's error code when exchanging an authorization code failed.

### Feeds, actions and saved drafts

- Required X and Mastodon responses to confirm likes, unlikes, reposts, bookmarks and other supported state changes before reporting success. Rejected error payloads returned with HTTP 200.
- Updated action flags and counters together after confirmed writes. Repeating a command kept counts unchanged, and counters stopped at their numeric limits.
- Preserved posts and pagination cursors from failed networks when another network refreshed successfully. Kept merged feeds in chronological order when only one network had more pages.
- Stopped pagination when a provider repeated its cursor, retained the returned posts and showed a notice. Treated empty cursors as the last page and rejected malformed or overflowing Mastodon search cursors.
- Distinguished rejected writes from requests whose outcome could not be confirmed. Retained the affected networks in saved drafts and warned users to check before retrying.
- Preserved uncertain-send warnings through composer autosave, dry runs, discarded edits and failed final draft updates. Included `sending` and `warning` in draft JSON when a send remained unconfirmed.
- Stopped Threads publication when container preparation failed, lost its connection or exhausted its polling limit.
- Kept Threads unavailable outside the demo while Meta approval was pending, including explicit network selection, login and saved defaults.

### Images and terminal output

- Bounded pending image downloads and cache metadata, and retried transient fetch failures with a delay capped at 30 seconds while an image remained visible.
- Limited local image reads to 20 MiB and API response reads to 8 MiB. Closed lost-wakeup windows when image and link-preview workers shut down.
- Cleaned network-controlled keys and values in the JSON inspector before rendering them in the terminal.
- Stopped treating arbitrary errors containing `Broken pipe` as successful commands. Closed output pipes still exited quietly through the output writer.
- Fixed the light logo so its off frame kept an opaque tile, and refined the existing reader spacing and network settings.

### Installation and release checks

- Added a Linux installer that selected a release package, verified its checksum and printed how to start SoCLI.
- Added macOS installer disk images for Apple Silicon and Intel, alongside the existing archives and Linux packages.
- Required both npm archives, both macOS disk images, every Linux package and the Homebrew formulas before publishing. Rejected empty artifacts and honored npm package-name overrides.
- Corrected the reusable web workflow path, rejected duplicate changelog version sections and stopped packaging when a browser bundle failed.
- Wired the documented optional npm publishing token while keeping GitHub trusted publishing as the default.
- Added contributing guidelines and issue and pull request templates. Moved the TUI tests into a dedicated module and expanded regression coverage for session expiry, rejected actions, offline recovery, cursor boundaries and durable drafts.
- Removed all 71 existing WebAssembly build warnings with platform-specific imports and helpers, and made warnings fail the WebAssembly CI jobs. Added an execution test of the compiled module for isolated readers, keyboard input, theme changes and resizing.
- Checked deadline and section-navigation arithmetic at numeric boundaries.

### Compatibility and verification limits

The review used fixtures and local HTTP servers. It did not send posts or actions to real accounts. X's private GraphQL interface can still change upstream, and a lost write response still requires checking the network before retrying. Large conversations retain each provider's existing depth and page limits. The [review record](https://github.com/massimodeluisa/socli/blob/v0.1.3/docs/REVIEW.md) describes the source coverage and remaining boundaries.

## [0.1.2] - 2026-10-26

- Named the product SoCLI in the reader, the site, and the docs.
- Showed the Threads exchange error instead of a bare 502.
- Added the AI readiness badge, an Agent Skills format badge, and a blinking logo on the README.
- Set a longer homepage title, a logo alt text, and HSTS.
- Forwarded a badge X sign-in to the private address in its state.
- Showed the current version on the homepage.
- Replaced the Open Graph image and linked the site icons to the brand files.
- Removed the invalid timeout from the reusable website job.
- Kept the usage line as socli on Windows, where the executable name is socli.exe.
- Wrote mouse-off sequences to the terminal instead of restoring a Windows console mode, and tested cookie permission errors by kind.
- Named the published npm packages in the contributor docs.
- Included npm's own error text when publication fails.
- Pinned GitHub Actions to commit SHAs, gated Windows in CI, and dropped the npm token so publication uses GitHub OIDC.
- Fixed Windows compilation by registering SIGHUP only on Unix and widened the watchdog restore bound.
- Bundled all six native executables into the soclish npm package and published the browser library as @massimodeluisa/soclib.
- Grouped Homebrew architectures under one block per operating system and removed a redundant version declaration.
- Added tag-gated deployment to the existing Cloudflare site worker and made the hosting target explicit.
- Resolved npm tarball paths before publication so npm did not interpret them as GitHub repositories.

## [0.1.1] - 2026-10-25

- Ran Linux smoke tests directly through QEMU so version and offline-feed arguments reached the executable.

- Made the help-output test follow the package version during releases.

- Fixed cross installation by verifying its archive against a pinned SHA-256 instead of a nonexistent checksum file.
- Added manual release-build verification without publication.
- Included the brand-page assets in clean website builds.

## [0.1.0] - 2026-10-25

- Added a tested Homebrew tap, a source formula for Core review, and independent Homebrew publication.

- Updated the website and installation guides to use npx and bunx and documented Homebrew Core availability.
- Made Homebrew tap publication optional so Core distribution did not block other release jobs.
- Fixed terminal and web sign-in for X, Bluesky, Threads, Mastodon, and dev/null.
- Served the web reader from the site worker and relayed only api.x.com.
- Updated tokio to 1.54.1 and rustls-webpki to 0.103.16.
- Added CI, Dependabot, and cargo-deny.
- Added tag-triggered native archives, Linux packages, a WebAssembly npm package, Homebrew updates, and website deployment.
- Added an npm launcher with platform packages for native `npx` installation and verified publishing retries.
- Added soclib with mounted and headless browser readers, isolated instances, host adapters, bundled assets, and state-preserving theme changes.
