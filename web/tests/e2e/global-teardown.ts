import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const PROJECT_ROOT = path.resolve(__dirname, '../../..')
const WEB_DEV = path.join(PROJECT_ROOT, 'scripts/bin/web-dev')

export default async function globalTeardown() {
  if (process.env.E2E_KEEP === '1') {
    console.log('[e2e] E2E_KEEP=1 — leaving web-dev stack running')
    return
  }
  const res = spawnSync('bash', [WEB_DEV, 'stop'], {
    cwd: PROJECT_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  if ((res.status ?? 0) !== 0) {
    console.warn('[e2e] web-dev stop returned non-zero; continuing anyway')
    if (res.stdout) console.warn(res.stdout)
    if (res.stderr) console.warn(res.stderr)
  }
}
