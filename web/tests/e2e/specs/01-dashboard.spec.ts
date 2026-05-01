import { test, expect } from '@playwright/test'

test.describe('Dashboard', () => {
  test('renders the four summary cards on /p/1', async ({ page }) => {
    await page.goto('/p/1')

    // Heading
    await expect(
      page.getByRole('heading', { name: /dashboard/i, level: 1 }),
    ).toBeVisible()

    // 1. Tasks-by-status card — one row per status (5 statuses).
    await expect(page.locator('[data-testid^="status-row-"]')).toHaveCount(5)

    // 2. Recently updated tasks — at least one entry from the seed.
    await expect(
      page.locator('[data-testid^="recent-task-"]').first(),
    ).toBeVisible()

    // 3. Ready tasks — the seed has multiple Todo tasks with all deps
    //    completed (e.g. tasks under the Auth refactor chain), so at least
    //    one should appear.
    await expect(
      page.locator('[data-testid^="ready-task-"]').first(),
    ).toBeVisible()

    // 4. Contracts — the seed has exactly 5.
    await expect(page.locator('[data-testid^="contract-row-"]')).toHaveCount(5)
  })

  test('AppHeader exposes the project / language / theme controls', async ({
    page,
  }) => {
    await page.goto('/p/1')
    await expect(
      page.getByRole('combobox', { name: /project|プロジェクト/i }),
    ).toBeVisible()
    await expect(
      page.getByRole('combobox', { name: /language|言語/i }),
    ).toBeVisible()
    // Ark UI Switch.Root renders a <label aria-label="…">; the underlying
    // checkbox has the dynamic Light/Dark name. Use the label.
    await expect(
      page.getByLabel(/toggle theme|テーマを切り替え/i),
    ).toBeVisible()
  })
})
