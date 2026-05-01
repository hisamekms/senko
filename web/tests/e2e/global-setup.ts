import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const PROJECT_ROOT = path.resolve(__dirname, '../../..')
const WEB_DEV = path.join(PROJECT_ROOT, 'scripts/bin/web-dev')

const SENKO_HEALTH_URL = 'http://127.0.0.1:3142/auth/config'
const VITE_HEALTH_URL = 'http://localhost:3000/'

function runWebDev(verb: 'status' | 'start' | 'stop' | 'reset'): {
  code: number
  stdout: string
  stderr: string
} {
  const result = spawnSync('bash', [WEB_DEV, verb], {
    cwd: PROJECT_ROOT,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  return {
    code: result.status ?? 1,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
  }
}

function isStackAlive(): boolean {
  const status = runWebDev('status')
  // Both processes must report a live PID. The script prints lines like
  // "  senko serve : running (pid=…, port=…)" / "  vite dev : running …".
  // Anything else ("not running" / "stale pid file") means dead.
  const lines = status.stdout.split('\n')
  const isRunning = (label: string): boolean => {
    const line = lines.find((l) => l.includes(label))
    if (!line) return false
    // Match the colon-separated state token; reject "not running".
    return /:\s*running\b/.test(line)
  }
  return isRunning('senko serve') && isRunning('vite dev')
}

async function waitForUrl(url: string, label: string, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs
  let lastErr: unknown = null
  while (Date.now() < deadline) {
    try {
      const res = await fetch(url, {
        signal: AbortSignal.timeout(3_000),
      })
      // Any HTTP response means the server is up — Vite serves 307s on /
      // and senko's /auth/config returns 200.
      if (res.status >= 100 && res.status < 600) return
    } catch (err) {
      lastErr = err
    }
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(
    `[e2e] ${label} did not become ready at ${url} within ${timeoutMs}ms (last error: ${String(lastErr)})`,
  )
}

export default async function globalSetup() {
  const fresh = process.env.E2E_FRESH === '1'
  const alive = isStackAlive()

  if (alive && !fresh) {
    console.log(
      '[e2e] reusing running web-dev stack (set E2E_FRESH=1 to force a fresh seed)',
    )
  } else {
    if (alive && fresh) {
      console.log('[e2e] E2E_FRESH=1 — resetting the running stack')
    } else {
      console.log('[e2e] starting web-dev stack')
    }
    const verb = alive || fresh ? 'reset' : 'start'
    const res = runWebDev(verb)
    if (res.code !== 0) {
      console.error(res.stdout)
      console.error(res.stderr)
      throw new Error(`[e2e] 'web-dev ${verb}' exited with code ${res.code}`)
    }
  }

  await waitForUrl(SENKO_HEALTH_URL, 'senko serve')
  await waitForUrl(VITE_HEALTH_URL, 'vite dev')
  console.log('[e2e] stack ready: senko=:3142 vite=:3000')
}
