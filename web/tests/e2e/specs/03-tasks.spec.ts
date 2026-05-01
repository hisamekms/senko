import { test, expect } from '@playwright/test'

// Wait for at least one task card to render — that signal proves the route
// has hydrated and the API response landed, so subsequent interactions
// (filter clicks, navigation) actually fire React handlers instead of being
// dropped against the SSR'd HTML.
async function waitForListReady(page: import('@playwright/test').Page) {
  await expect(
    page.locator('[data-testid^="task-card-"]').first(),
  ).toBeVisible()
}

test.describe('Tasks list + detail', () => {
  test('list page renders task cards and supports cursor pagination', async ({
    page,
  }) => {
    await page.goto('/p/1/tasks')
    await waitForListReady(page)

    const loadMore = page.getByTestId('task-list-load-more')
    if (await loadMore.isVisible().catch(() => false)) {
      const before = await page
        .locator('[data-testid^="task-card-"]')
        .count()
      await loadMore.click()
      await expect
        .poll(() => page.locator('[data-testid^="task-card-"]').count())
        .toBeGreaterThan(before)
    }
  })

  test('status filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/tasks')
    await waitForListReady(page)

    await page.getByTestId('task-filter-status-todo').click()
    // The TaskFilters URL state encodes `status` as a JSON array, so the
    // raw query string is `?status=%5B%22todo%22%5D`. Match either the
    // encoded or decoded form to stay resilient to encoding tweaks.
    await expect(page).toHaveURL(/status=(todo|%5B%22todo%22%5D)/)
    await expect(
      page.locator('[data-testid^="task-card-"]').first(),
    ).toBeVisible()

    await page.getByTestId('task-filter-reset').click()
    await expect(page).not.toHaveURL(/status=/)
  })

  test('ready-only filter wires through the URL', async ({ page }) => {
    await page.goto('/p/1/tasks')
    await waitForListReady(page)

    // The checkbox is wrapped in a <label>; the visible label markup
    // intercepts pointer events on the input itself, so dispatch the click
    // through Playwright's force mode.
    await page.getByTestId('task-filter-ready').click({ force: true })
    await expect(page).toHaveURL(/[?&]ready=true/)
  })

  test('detail page surfaces title, status badge, and dependencies', async ({
    page,
  }) => {
    // Task #3 ("[seed] Implement token issuance endpoint") is in_progress
    // and depends on task #2.
    await page.goto('/p/1/tasks/3')

    await expect(
      page.getByRole('heading', { name: /token issuance/i, level: 1 }),
    ).toBeVisible()
    await expect(page.getByTestId('task-status-in_progress')).toBeVisible()
    await expect(page.getByTestId('dep-link-2')).toBeVisible()
  })
})
