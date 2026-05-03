import { test, expect } from '@playwright/test'

// Verifies the security-headers request middleware (web/src/start.ts +
// web/src/utils/security/csp.ts) actually attaches headers and that the
// nonce flows into the SSR HTML so the inline themeBootstrap script is
// allowed under the strict CSP. Dev runs the e2e suite, so we expect:
//   - Content-Security-Policy-Report-Only (NOT enforcing CSP)
//   - NO Strict-Transport-Security (HSTS is prod-only)
//   - NO X-Frame-Options (CSP `frame-ancestors 'none'` covers it for all
//     browsers we support)
// Five base header families are always present (Referrer-Policy,
// X-Content-Type-Options, Permissions-Policy, Cross-Origin-Opener-Policy,
// Cross-Origin-Resource-Policy); CSP variant and HSTS depend on environment.

test.describe('Security headers', () => {
  test('dev sends 5 base security headers + Report-Only CSP and no HSTS / X-Frame-Options', async ({
    request,
  }) => {
    const res = await request.get('/p/1', { maxRedirects: 0 })

    expect(res.headers()['referrer-policy']).toBe(
      'strict-origin-when-cross-origin',
    )
    expect(res.headers()['x-content-type-options']).toBe('nosniff')
    const permissions = res.headers()['permissions-policy'] ?? ''
    for (const feature of [
      'camera',
      'microphone',
      'geolocation',
      'payment',
      'usb',
      'midi',
      'serial',
      'bluetooth',
      'magnetometer',
      'accelerometer',
      'gyroscope',
      'interest-cohort',
    ]) {
      expect(permissions).toContain(`${feature}=()`)
    }

    expect(res.headers()['cross-origin-opener-policy']).toBe('same-origin')
    expect(res.headers()['cross-origin-resource-policy']).toBe('same-origin')

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

// CSP enforce-readiness: under `style-src 'self'` (no 'unsafe-inline'),
// senko-web's own components must render dynamic dimensions through
// classNames (Panda CSS), not via the JSX `style={...}` attribute. The
// progress fills are the canonical dynamic-width sink — assert that they
// no longer carry an inline `style` attribute. Third-party libraries
// (xyflow, ark-ui, react-markdown) may still emit inline styles internally;
// those are out of scope here, and the `securitypolicyviolation` event
// reports `sourceFile` as the page URL for inline-attribute violations,
// so it cannot be used to programmatically distinguish own from third-party.
test.describe('CSP enforce-readiness for own components', () => {
  test('contract fill bars on /p/1 render without inline style attribute', async ({
    page,
  }) => {
    // `domcontentloaded` rather than `networkidle`: the dev stack keeps
    // long-running connections (HMR / SSE) so networkidle never settles.
    // The explicit row wait below gates on the actual data we care about.
    await page.goto('/p/1', { waitUntil: 'domcontentloaded' })

    // Wait for at least one contract row — guarantees ContractsCard has
    // finished its data fetch and rendered its fill bars.
    const firstRow = page.locator('[data-testid^="contract-row-"]').first()
    await expect(firstRow).toBeVisible()

    // Each contract row contains a fill bar: `<div role="progressbar"><div .../></div>`.
    // Count any fill divs that still carry an inline `style` attribute — should
    // be zero now that the dynamic width comes from FILL_WIDTH_BUCKETS.
    const offenders = await page
      .locator(
        '[data-testid^="contract-row-"] [role="progressbar"] > div[style]',
      )
      .count()
    expect(offenders).toBe(0)
  })

  test('contract progress bar on /p/1/contracts/1 renders without inline style attribute', async ({
    page,
  }) => {
    // See note above on `domcontentloaded` vs `networkidle`.
    await page.goto('/p/1/contracts/1', { waitUntil: 'domcontentloaded' })

    // ContractProgressBar in the contract detail header.
    await expect(page.getByRole('progressbar').first()).toBeVisible()

    const offenders = await page
      .locator('[role="progressbar"] > div[style]')
      .count()
    expect(offenders).toBe(0)
  })
})
