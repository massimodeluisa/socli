# socli for npm

This package runs the native socli terminal client. npm selects the executable for your operating system and CPU from a set of optional packages, all pinned to the same release version. The launcher verifies the executable's SHA-256 and forwards your arguments, terminal input, output, and exit status.

After the package is published, run it with Node.js 22 or newer:

```sh
npx socli@latest --help
bunx socli@latest --demo
```

To run the launcher with Bun itself, use `bunx --bun socli@latest --demo`.

To keep the command installed:

```sh
npm install --global socli
socli --help
```

These commands become available after publication. Publishing requires ownership of the unscoped `socli` name and its platform package names. If the release overrides the default name, substitute that name in the commands.

The supported platforms are macOS Intel and Apple Silicon, and Linux ARMv7, ARM64, x86, and x64. Linux packages contain musl executables that also run on glibc systems. Windows builds are not enabled. An ARM installation requires ARMv7 or newer and a matching Node.js build.

Keep optional dependencies enabled when installing. If the launcher reports a missing platform package, reinstall with `npm install --include=optional`. The package has no postinstall script and does not download an executable when you launch it. npm downloads the matching package during installation; the launcher uses that installed executable.

The native client uses the same credentials and configuration as a binary installed through Homebrew or downloaded from GitHub. The separate browser package, `soclib` by default, contains the WebAssembly engine for a web host.
