import { test, expect } from '@playwright/test'

// Regression guard for Task #425: ark-ui's internal components emit a few
// inline `style="..."` attributes (visually-hidden Select / Switch inputs,
// Floating UI portal positioning). After we relax CSP with
// `style-src-attr 'unsafe-inline'`, navigating a page that exercises these
// components must produce ZERO violations whose effectiveDirective is
// `style-src-attr`.
//
// Dev runs the e2e suite under Report-Only CSP, but
// `securitypolicyviolation` events fire on the document in both Report-Only
// and enforce mode — so the assertion is meaningful here even though the
// browser does not block the styles in dev.

interface CspViolationRecord {
  effectiveDirective: string
  blockedURI: string
  sourceFile: string
}

declare global {
  interface Window {
    __cspViolations: CspViolationRecord[]
  }
}

test.describe('CSP style-src-attr (ark-ui inline-style relaxation)', () => {
  test('no style-src-attr violations on /p/1/contracts/1', async ({ page }) => {
    // Install the listener BEFORE navigation so the very first violation
    // fired during initial render is captured.
    await page.addInitScript(() => {
      window.__cspViolations = []
      window.addEventListener('securitypolicyviolation', (e) => {
        window.__cspViolations.push({
          effectiveDirective: e.effectiveDirective,
          blockedURI: e.blockedURI,
          sourceFile: e.sourceFile,
        })
      })
    })

    // Use `domcontentloaded` rather than `networkidle`: the dev stack keeps
    // long-running connections (HMR / SSE) so networkidle never settles.
    // The 3-second wait below gives the contract-detail data fetch and
    // any deferred portal / popper render a chance to fire.
    await page.goto('/p/1/contracts/1', {
      waitUntil: 'domcontentloaded',
      timeout: 30_000,
    })
    await page.waitForTimeout(3000)

    const violations = await page.evaluate(() => window.__cspViolations)
    const styleAttrViolations = violations.filter(
      (v) => v.effectiveDirective === 'style-src-attr',
    )

    expect(
      styleAttrViolations,
      `Unexpected style-src-attr violations: ${JSON.stringify(
        styleAttrViolations,
        null,
        2,
      )}`,
    ).toHaveLength(0)
  })
})
