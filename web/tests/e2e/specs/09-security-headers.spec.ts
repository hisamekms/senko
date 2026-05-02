import { test, expect } from '@playwright/test'

// Verifies the security-headers request middleware (web/src/start.ts +
// web/src/utils/security/csp.ts) actually attaches headers and that the
// nonce flows into the SSR HTML so the inline themeBootstrap script is
// allowed under the strict CSP. Dev runs the e2e suite, so we expect:
//   - Content-Security-Policy-Report-Only (NOT enforcing CSP)
//   - NO Strict-Transport-Security (HSTS is prod-only)
//   - NO X-Frame-Options (CSP `frame-ancestors 'none'` covers it for all
//     browsers we support)
// Three base header families are always present (Referrer-Policy,
// X-Content-Type-Options, Permissions-Policy); CSP variant and HSTS depend
// on environment.

test.describe('Security headers', () => {
  test('dev sends 3 base security headers + Report-Only CSP and no HSTS / X-Frame-Options', async ({
    request,
  }) => {
    const res = await request.get('/p/1', { maxRedirects: 0 })

    expect(res.headers()['referrer-policy']).toBe(
      'strict-origin-when-cross-origin',
    )
    expect(res.headers()['x-content-type-options']).toBe('nosniff')
    const permissions = res.headers()['permissions-policy'] ?? ''
    expect(permissions).toContain('camera=()')
    expect(permissions).toContain('microphone=()')
    expect(permissions).toContain('geolocation=()')

    // Dev uses Report-Only mode so HMR violations are logged but not blocked.
    const csp = res.headers()['content-security-policy-report-only']
    expect(csp).toBeTruthy()
    expect(csp).toMatch(/script-src [^;]*'nonce-[A-Za-z0-9_-]+'/)
    expect(csp).toContain("frame-ancestors 'none'")
    expect(csp).toContain("object-src 'none'")
    expect(csp).toContain("base-uri 'self'")
    expect(csp).toContain("form-action 'self'")

    // No enforcing CSP in dev.
    expect(res.headers()['content-security-policy']).toBeUndefined()
    // HSTS must NOT be sent in dev (localhost shouldn't be pinned).
    expect(res.headers()['strict-transport-security']).toBeUndefined()
    // X-Frame-Options must NOT be sent — frame-ancestors in CSP supersedes it.
    expect(res.headers()['x-frame-options']).toBeUndefined()
  })

  test('SSR HTML contains the csp-nonce meta and applies the nonce to the inline themeBootstrap script', async ({
    request,
  }) => {
    const res = await request.get('/p/1', { maxRedirects: 0 })
    const html = await res.text()

    // TanStack Router auto-injects this meta when router.options.ssr.nonce is
    // set, which the client uses for hydration scripts.
    const meta = html.match(
      /<meta[^>]+property=["']csp-nonce["'][^>]+content=["']([A-Za-z0-9_-]+)["']/i,
    )
    expect(meta).not.toBeNull()
    const nonce = meta![1]

    const cspNonce = res
      .headers()
      ['content-security-policy-report-only']!.match(
        /'nonce-([A-Za-z0-9_-]+)'/,
      )?.[1]
    expect(cspNonce).toBe(nonce)

    // The inline themeBootstrap script must carry the nonce, otherwise it
    // would be blocked under enforcing CSP. Match a script whose body
    // contains the unique themeBootstrap marker `senko.web.theme`.
    const themeScript = html.match(
      /<script[^>]*>[^<]*senko\.web\.theme[^<]*<\/script>/i,
    )
    expect(themeScript).not.toBeNull()
    expect(themeScript![0]).toMatch(
      new RegExp(`nonce=['"]${nonce}['"]`, 'i'),
    )
  })
})
