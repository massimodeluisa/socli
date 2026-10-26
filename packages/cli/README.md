# soclish

soclish installs the native SoCLI terminal client. The command is `socli`. One package contains the prebuilt executables for macOS Intel and Apple Silicon, and Linux ARMv7, ARM64, x86, and x64. The launcher selects the matching bundled executable, verifies its SHA-256, and forwards your arguments, terminal input, output, and exit status.

Run it with Node.js 22 or newer:

```sh
npx soclish@latest --demo
bunx soclish@latest --demo
```

Use `bunx --bun soclish@latest --demo` to run the launcher with Bun itself.

To keep the command installed:

```sh
npm install --global soclish
socli --demo
```

The package includes all six executables and has no runtime npm dependencies. It installs without Rust or a postinstall script. Linux executables use musl and also run on glibc systems. ARM requires ARMv7 or newer and a matching Node.js build. Windows builds remain disabled.

The native client uses the same credentials and configuration as a binary installed through Homebrew or downloaded from GitHub. The browser library, `@massimodeluisa/soclib`, contains the WebAssembly engine, worker, renderer, styles, and fonts for web applications.
