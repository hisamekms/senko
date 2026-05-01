import { spawn, type ChildProcess } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { test, expect, request } from '@playwright/test'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// `WEB_DEV_AUTH_BYPASS` is read at server start (web/src/routes/__root.tsx
// and web/src/routes/api/senko/$.ts) so the bypass-on dev server can never
// reproduce the unauthenticated-user behavior. We spin up a second Vite
// instance on port 3001 with bypass *off* for this single spec, and tear
// it down afterwards.

const WEB_DIR = path.resolve(__dirname, '../../..')
const SECONDARY_PORT = 3001
const SECONDARY_BASE = `http://localhost:${SECONDARY_PORT}`

let viteProc: ChildProcess | undefined

async function waitFor(url: string, timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs
  let lastErr: unknown = null
  while (Date.now() < deadline) {
    try {
      const r = await fetch(url, { signal: AbortSignal.timeout(3_000) })
      if (r.status >= 100 && r.status < 600) return
    } catch (err) {
      lastErr = err
    }
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(
    `secondary vite did not respond on ${url} within ${timeoutMs}ms (last error: ${String(lastErr)})`,
  )
}

test.describe.configure({ mode: 'serial' })

test.describe('Auth redirect', () => {
  test.beforeAll(async () => {
    const viteBin = path.join(WEB_DIR, 'node_modules/.bin/vite')

    viteProc = spawn(viteBin, ['dev', '--port', String(SECONDARY_PORT)], {
      cwd: WEB_DIR,
      env: {
        ...process.env,
        // Auth.js needs these to construct callback URLs and sign cookies,
        // even though we never actually complete a sign-in in this spec.
        AUTH_SECRET: 'e2e-test-secret-padding-padding-padding',
        AUTH_URL: `${SECONDARY_BASE}/api/auth`,
        AUTH_OIDC_ISSUER: 'http://localhost:1/oidc-not-reachable',
        AUTH_OIDC_CLIENT_ID: 'e2e-client',
        AUTH_OIDC_CLIENT_SECRET: 'e2e-client-secret',
        SENKO_API_BASE_URL: 'http://127.0.0.1:3142',
        WEB_DEV_PORT: String(SECONDARY_PORT),
        // Explicit unset of the bypass — this is the whole point of the
        // secondary instance.
        WEB_DEV_AUTH_BYPASS: 'false',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    viteProc.stdout?.on('data', () => {})
    viteProc.stderr?.on('data', () => {})

    await waitFor(`${SECONDARY_BASE}/`)
  })

  test.afterAll(async () => {
    if (viteProc && viteProc.pid) {
      viteProc.kill('SIGTERM')
      await new Promise((r) => setTimeout(r, 500))
      if (!viteProc.killed) viteProc.kill('SIGKILL')
    }
  })

  test('unauthenticated visit to /p/1 redirects to /login', async () => {
    // The protected pages issue a TanStack Router redirect that the server
    // realises as an HTTP 3xx. Ask Playwright's request context to NOT
    // follow it so we can inspect the Location header.
    const ctx = await request.newContext({ baseURL: SECONDARY_BASE })
    try {
      const res = await ctx.get('/p/1', { maxRedirects: 0 })
      expect(res.status()).toBeGreaterThanOrEqual(300)
      expect(res.status()).toBeLessThan(400)
      const loc = res.headers().location ?? ''
      expect(loc).toMatch(/\/login/)
    } finally {
      await ctx.dispose()
    }
  })

  test('authenticated visit to /login redirects back to /', async ({
    page,
  }) => {
    // On the bypass-on instance (port 3000), a fake session is injected at
    // the root, so the /login route's beforeLoad should bounce to /.
    await page.goto('http://localhost:3000/login')
    await expect(page).toHaveURL(/^http:\/\/localhost:3000\/(?:p\/\d+\/?)?$/)
  })
})
