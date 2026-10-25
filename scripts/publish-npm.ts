import { createHash } from 'node:crypto'
import { execFileSync, spawnSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { join } from 'node:path'

export async function npmPublishPlan(assets: string) {
  const packages = []
  for (const name of (await readdir(assets)).filter((name) => name.endsWith('.tgz'))) {
    const path = join(assets, name)
    const manifest = JSON.parse(execFileSync('tar', ['-xOf', path, 'package/package.json'], { encoding: 'utf8' }))
    packages.push({ path, manifest, integrity: `sha512-${createHash('sha512').update(await readFile(path)).digest('base64')}` })
  }
  const launcher = packages.find((item) => item.manifest.bin?.socli)
  if (!launcher) throw new Error('npm distribution has no socli launcher')
  if (new Set(packages.map((item) => item.manifest.name)).size !== packages.length) throw new Error('npm package names must be distinct')
  if (packages.some((item) => item.manifest.version !== launcher.manifest.version)) throw new Error('all npm packages must have the same version')
  for (const [name, version] of Object.entries(launcher.manifest.optionalDependencies ?? {})) {
    if (!packages.some((item) => item.manifest.name === name && item.manifest.version === version)) {
      throw new Error(`npm distribution is missing ${name}@${version}`)
    }
  }
  return packages.sort((a, b) => Number(Boolean(a.manifest.bin?.socli)) - Number(Boolean(b.manifest.bin?.socli)) || a.manifest.name.localeCompare(b.manifest.name))
}

export function publicationState(result: { status: number | null; stdout: string; stderr: string }, integrity: string): 'published' | 'missing' {
  if (result.status === 0) {
    if (JSON.parse(result.stdout) !== integrity) throw new Error('npm version already exists with different package bytes')
    return 'published'
  }
  let error
  try { error = JSON.parse(result.stdout).error } catch {}
  if (error?.code === 'E404') return 'missing'
  throw new Error(`could not check the npm package: ${result.stderr || result.stdout}`)
}

if (import.meta.main) {
  const directory = process.argv[2]
  const tag = process.env.NPM_TAG
  if (!directory || !['latest', 'next'].includes(tag ?? '')) throw new Error('usage: NPM_TAG=latest|next bun scripts/publish-npm.ts ASSETS')
  for (const item of await npmPublishPlan(directory)) {
    const specifier = `${item.manifest.name}@${item.manifest.version}`
    const result = spawnSync('npm', ['view', specifier, 'dist.integrity', '--json'], { encoding: 'utf8' })
    if (result.error) throw result.error
    if (publicationState(result, item.integrity) === 'published') {
      console.log(`already published: ${specifier}`)
      continue
    }
    execFileSync('npm', ['publish', item.path, '--access', 'public', '--tag', tag], { stdio: 'inherit' })
  }
}
