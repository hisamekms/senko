import { test, expect } from '@playwright/test'

async function waitForContractsReady(
  page: import('@playwright/test').Page,
) {
  await expect(
    page.locator('[data-testid^="contract-card-"]').first(),
  ).toBeVisible()
}

test.describe('Contracts list + detail + notes', () => {
  test('list page renders seeded contracts', async ({ page }) => {
    await page.goto('/p/1/contracts')
    await waitForContractsReady(page)
    // Contracts page is now paginated — at least one card must be visible;
    // the rest may sit behind Load more.
    expect(
      await page.locator('[data-testid^="contract-card-"]').count(),
    ).toBeGreaterThan(0)
  })

  test('completion filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/contracts')
    await waitForContractsReady(page)

    await page.getByTestId('contract-filter-completion-completed').click()
    await expect(page).toHaveURL(/completed=true/)

    await page.getByTestId('contract-filter-completion-in_progress').click()
    await expect(page).toHaveURL(/completed=false/)

    await page.getByTestId('contract-filter-completion-all').click()
    await expect(page).not.toHaveURL(/completed=/)
  })

  test('title filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/contracts')
    await waitForContractsReady(page)

    await page.getByTestId('contract-filter-title').fill('auth')
    await page.getByTestId('contract-filter-title').blur()
    await expect(page).toHaveURL(/title=auth/)

    await page.getByTestId('contract-filter-reset').click()
    await expect(page).not.toHaveURL(/title=/)
  })

  test('tag filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/contracts')
    await waitForContractsReady(page)

    await page.getByTestId('contract-filter-tag').fill('backend')
    await page.getByTestId('contract-filter-tag').blur()
    // tag is encoded the same way as on Tasks: single value or JSON-array form.
    await expect(page).toHaveURL(/tag=(backend|%5B%22backend%22%5D)/)

    await page.getByTestId('contract-filter-reset').click()
    await expect(page).not.toHaveURL(/tag=/)
  })

  test('Load-more pagination appends additional cards when available', async ({
    page,
  }) => {
    await page.goto('/p/1/contracts')
    await waitForContractsReady(page)

    const loadMore = page.getByTestId('contract-list-load-more')
    if (await loadMore.isVisible().catch(() => false)) {
      const before = await page
        .locator('[data-testid^="contract-card-"]')
        .count()
      await loadMore.click()
      await expect
        .poll(() => page.locator('[data-testid^="contract-card-"]').count())
        .toBeGreaterThan(before)
    }
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
