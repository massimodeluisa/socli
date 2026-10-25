# Changelog

Release notes are kept here. Move entries from `[Unreleased]` into a dated version section before tagging a release.

## [Unreleased]

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
