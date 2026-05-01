import { test, expect } from '@playwright/test'

test.describe('Contracts list + detail + notes', () => {
  test('list page renders all 5 seeded contracts', async ({ page }) => {
    await page.goto('/p/1/contracts')
    await expect(page.locator('[data-testid^="contract-card-"]')).toHaveCount(5)
  })

  test('detail page renders DoD checklist and notes timeline', async ({
    page,
  }) => {
    // Contract #1 ("[seed] Auth refactor") has DoD items, multiple notes,
    // and related tasks — exercises the whole detail surface.
    await page.goto('/p/1/contracts/1')

    await expect(
      page.getByRole('heading', { name: /auth refactor/i, level: 1 }),
    ).toBeVisible()

    // DoD checklist is rendered with `aria-label="checked"|"unchecked"` per item.
    const dodItems = page.locator('[aria-label="checked"], [aria-label="unchecked"]')
    expect(await dodItems.count()).toBeGreaterThan(0)

    // Notes timeline — at least one seeded note for contract 1.
    await expect(
      page.locator('[data-testid^="contract-note-"]').first(),
    ).toBeVisible()
  })
})
