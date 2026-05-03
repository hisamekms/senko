# CSP `style-src-attr` and ark-ui inline-style attributes

## Problem

After Task #424 dropped `'unsafe-inline'` from CSP `style-src`, prod
(enforce mode) reports four classes of violations on every senko-web page
that uses `@ark-ui/react` components — but **none of them come from
senko-web's own JSX**:

| # | Element | Inline `style` value | Origin |
|---|---|---|---|
| 1 | `<select>` | `border:0; clip:rect(0,0,0,0); height:1px; …` | `Select` visually-hidden native control |
| 2 | `<input>` | `border:0;clip:rect(0 0 0 0);height:1px; …` | `Switch` visually-hidden checkbox |
| 3 | `<div>` | `position:absolute` | Popper / Portal positioning |
| 4 | `<div>` | `position:absolute; isolation:isolate; width:var(--reference-width); transform:translate3d(...); z-index:var(--z-index)` | Floating UI (Popover / Combobox) positioning |

Real XSS risk is ≈ 0 — every value is either a fixed library constant or
a computed coordinate; no attacker-controlled input flows into the
`style` attribute. But running a `report-uri` collects ~4 violations per
page-view and can mask real issues, and stylistic side effects are
visible if any of these styles end up dropped.

## Solution: `style-src-attr 'unsafe-inline'`

`Content-Security-Policy` Level 3 splits `style-src` into two narrower
directives:

- `style-src-elem` — controls `<style>` and `<link rel="stylesheet">`
  (and CSSOM stylesheet construction).
- `style-src-attr` — controls inline `style="…"` *attributes* only.

When a directive is **omitted**, browsers fall back to `style-src`. So
the targeted relaxation is:

```ts
const directives = [
  // …
  "style-src 'self'",
  "style-src-attr 'unsafe-inline'",
  // (style-src-elem deliberately not emitted)
]
```

Behaviour:

- `<style>` blocks and `<link rel="stylesheet">` → still gated by
  `style-src 'self'` (no `'unsafe-inline'`, no foreign origins).
- Inline `style="…"` attributes → permitted everywhere. Acceptable
  because the dangerous sinks (text content, attribute values) are
  guarded by React's standard escaping plus our own component layer; no
  user-controlled string ever reaches a `style` attribute in
  senko-web.

This is the implementation in `web/src/utils/security/csp.ts`. The
unit suite (`csp.test.ts`) pins three invariants: `style-src-attr
'unsafe-inline'` is present, `style-src 'self'` is still present, and
`style-src-elem` is **not** emitted (so the fallback chain stays
intact).

## Approaches considered and rejected

| Option | Why not |
|---|---|
| **A.** `style-src-attr 'unsafe-hashes' 'sha256-…'` (enumerate hashes) | Nodes #3 and #4 contain dynamic values (`var(--reference-width)`, `translate3d(<Xpx>,<Ypx>,…)`). Hash-based allowlists are static — they cannot match values computed at runtime. |
| **C.** Replace ark-ui with a different headless library / hand-rolled primitives | Requires re-implementing a11y-correct Switch / Select / Popover / Combobox. Out of scope (massive effort for a near-zero-risk benefit). |
| **D.** Upstream PR to ark-ui to switch positioning to CSS variables / classes | We cannot block on a third-party maintainer. Recording the limitation here is the available action. |
| **E.** Split into `style-src-elem` (strict hashes) + `style-src-attr` (the allowed set) | Same dynamic-value problem as A — the popper coordinates still need `'unsafe-inline'`, and once `style-src-attr 'unsafe-inline'` is in we have effectively reduced to **B**. |

## Reproducing the inline-style census

The following Playwright spec scans every page for `[style]` attributes
and prints what is actually rendered. Useful when ark-ui (or any other
dependency) adds new inline-style sinks and you need to triage whether
they are tolerable.

```ts
import { test } from '@playwright/test'

test('inspect inline style attributes per page', async ({ page }) => {
  test.setTimeout(120_000)
  for (const path of ['/p/1', '/p/1/graph', '/p/1/tasks/1', '/p/1/contracts/1']) {
    await page.goto(path, { waitUntil: 'domcontentloaded', timeout: 30_000 })
    await page.waitForTimeout(3000)
    const styleNodes = await page.evaluate(() => {
      const out: Array<Record<string, string>> = []
      document.querySelectorAll<HTMLElement>('[style]').forEach((el) => {
        let parent: Element | null = el.parentElement
        let nearestTestId = ''
        while (parent) {
          const tid = parent.getAttribute('data-testid')
          if (tid) { nearestTestId = tid; break }
          parent = parent.parentElement
        }
        out.push({
          tag: el.tagName.toLowerCase(),
          classes: (el.className || '').toString().slice(0, 100),
          style: (el.getAttribute('style') ?? '').slice(0, 200),
          testId: el.getAttribute('data-testid') ?? '',
          nearestTestId,
        })
      })
      return out
    })
    console.log(`\n=== ${path} (${styleNodes.length} nodes) ===`)
    for (const n of styleNodes) console.log(JSON.stringify(n))
  }
})
```

Drop it into `web/tests/e2e/specs/_dom-inline-style-census.spec.ts`,
run `mise run e2e -- _dom-inline-style-census`, and read the captured
log to classify each `[style]` node as: senko-web JSX (must be
migrated), third-party fixed value (acceptable under
`style-src-attr 'unsafe-inline'`), or third-party dynamic value (same).

## References

- Task #425 (this change), Task #424 (`style-src 'self'` lock-in).
- `web/src/utils/security/csp.ts` — `buildCspHeader` directive list.
- `web/src/utils/security/csp.test.ts` — `style-src-attr` describe block.
- `web/tests/e2e/specs/10-csp-style-src-attr.spec.ts` — runtime
  regression guard.
- W3C CSP3 — *Directive fallback list*: a missing
  `style-src-elem`/`-attr` falls through to `style-src`, then to
  `default-src`.
