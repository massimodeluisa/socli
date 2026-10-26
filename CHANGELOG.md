# Changelog

Release notes are kept here. Move entries from `[Unreleased]` into a dated version section before tagging a release.

## [Unreleased]

- Returned the one-hour Threads token when the 60-day swap fails, and included Meta's error code on a failed code exchange.

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
