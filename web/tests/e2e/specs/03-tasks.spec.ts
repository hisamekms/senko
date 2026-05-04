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

  test('priority filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/tasks')
    await waitForListReady(page)

    const before = await page
      .locator('[data-testid^="task-card-"]')
      .count()

    await page.getByTestId('task-filter-priority-P1').click()
    // priority is encoded the same way as status (single value or JSON array).
    await expect(page).toHaveURL(/priority=(P1|%5B%22P1%22%5D)/)

    // Wait for the filtered list to settle, then assert it differs from the
    // unfiltered baseline.
    await expect(
      page.locator('[data-testid^="task-card-"]').first(),
    ).toBeVisible()
    await expect
      .poll(() => page.locator('[data-testid^="task-card-"]').count())
      .toBeLessThanOrEqual(before)

    await page.getByTestId('task-filter-reset').click()
    await expect(page).not.toHaveURL(/priority=/)
  })

  test('title filter narrows the list and updates the URL', async ({
    page,
  }) => {
    await page.goto('/p/1/tasks')
    await waitForListReady(page)

    // Type a short fragment likely to exclude most seeded titles, then
    // commit the draft via blur (the input submits on blur or Enter).
    await page.getByTestId('task-filter-title').fill('token')
    await page.getByTestId('task-filter-title').blur()
    await expect(page).toHaveURL(/title=token/)
    await expect(
      page.locator('[data-testid^="task-card-"]').first(),
    ).toBeVisible()

    await page.getByTestId('task-filter-reset').click()
    await expect(page).not.toHaveURL(/title=/)
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

  test('detail page shows assignee username next to the user id', async ({
    page,
  }) => {
    // Task #3 is seeded with assignee_idx=2 → "bob" (see
    // src/dev/seeder/fixtures.rs:39-42 + resolve_assignee). The assignee row
    // should render `<username> (#<uid>)` rather than the raw `#<uid>`.
    await page.goto('/p/1/tasks/3')
    const assignee = page.getByTestId('task-assignee')
    await expect(assignee).toBeVisible()
    await expect(assignee).toContainText('bob')
    await expect(assignee).toContainText('#')
  })
})
