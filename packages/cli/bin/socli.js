#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { readFileSync, realpathSync } from 'node:fs'
import { createRequire } from 'node:module'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

export function selectPlatform(platform, arch, armVersion) {
  const key = `${platform}-${arch}`
  if (!['darwin-x64', 'darwin-arm64', 'linux-arm', 'linux-arm64', 'linux-ia32', 'linux-x64'].includes(key)) {
    throw new Error(`socli does not have an npm binary for ${key}; Windows builds are not enabled yet`)
  }
  if (arch === 'arm' && (!Number.isFinite(Number(armVersion)) || Number(armVersion) < 7)) {
    throw new Error('socli requires ARMv7 or newer; use an ARMv7 Node.js build')
  }
  return key
}

export function resolveBinary(platforms, platform, arch, armVersion, resolve) {
  const key = selectPlatform(platform, arch, armVersion)
  const entry = platforms[key]
  if (!entry) throw new Error(`socli release is missing binary metadata for ${key}`)
  let binary
  try {
    binary = resolve(`${entry.package}/bin/socli`)
  } catch {
    throw new Error(`socli needs ${entry.package}; reinstall with optional dependencies enabled (npm install --include=optional)`)
  }
  const hash = createHash('sha256').update(readFileSync(binary)).digest('hex')
  if (hash !== entry.sha256) throw new Error(`socli binary checksum failed for ${entry.package}; reinstall the package`)
  return binary
}

function main() {
  try {
    const platforms = JSON.parse(readFileSync(new URL('../platforms.json', import.meta.url), 'utf8'))
    const require = createRequire(import.meta.url)
    const binary = resolveBinary(platforms, process.platform, process.arch, process.config.variables.arm_version, require.resolve)
    const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })
    if (result.error) throw result.error
    if (result.signal) process.kill(process.pid, result.signal)
    else process.exitCode = result.status ?? 1
  } catch (error) {
    console.error(error instanceof Error ? error.message : 'could not start socli')
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main()
