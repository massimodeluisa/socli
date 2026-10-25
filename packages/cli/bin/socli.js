#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { readFileSync, realpathSync } from 'node:fs'
import { join } from 'node:path'
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

export function resolveBinary(platforms, platform, arch, armVersion, root) {
  const key = selectPlatform(platform, arch, armVersion)
  const entry = platforms[key]
  if (!entry || entry.path !== `native/${key}/socli`) throw new Error(`socli release is missing binary metadata for ${key}`)
  const binary = join(root, entry.path)
  let bytes
  try {
    bytes = readFileSync(binary)
  } catch {
    throw new Error(`socli release is missing its bundled ${key} executable; reinstall soclish`)
  }
  const hash = createHash('sha256').update(bytes).digest('hex')
  if (hash !== entry.sha256) throw new Error(`socli binary checksum failed for ${key}; reinstall soclish`)
  return binary
}

function main() {
  try {
    const platforms = JSON.parse(readFileSync(new URL('../platforms.json', import.meta.url), 'utf8'))
    const root = fileURLToPath(new URL('../', import.meta.url))
    const binary = resolveBinary(platforms, process.platform, process.arch, process.config.variables.arm_version, root)
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
