import { test, expect } from '@playwright/test'

test.describe('Project switching', () => {
  test('switches from project 1 to project 2 via the header switcher', async ({
    page,
  }) => {
    await page.goto('/p/1')

    // Open the Ark UI Select trigger by its accessible label.
    const trigger = page.getByRole('combobox', { name: /project|プロジェクト/i })
    await trigger.click()

    // Ark UI renders Select options into a Portal — query via the page,
    // not inside the trigger's subtree. Match the seeded secondary project
    // by its known title.
    await page
      .getByRole('option', { name: '[seed] Secondary' })
      .click()

    await expect(page).toHaveURL(/\/p\/2(?:\/|$|\?)/)

    // The destination dashboard should render — recent-tasks card surfaces
    // tasks from project 2 (the seeder put 3 there).
    await expect(
      page.getByRole('heading', { name: /dashboard/i, level: 1 }),
    ).toBeVisible()
    await expect(
      page.locator('[data-testid^="recent-task-"]').first(),
    ).toBeVisible()
  })

  test('lists at least two projects in the switcher', async ({ page }) => {
    await page.goto('/p/1')
    await page
      .getByRole('combobox', { name: /project|プロジェクト/i })
      .click()
    // The seed contributes the bootstrap default project plus '[seed] Secondary'.
    const options = page.getByRole('option')
    await expect(options).toHaveCount(await options.count())
    expect(await options.count()).toBeGreaterThanOrEqual(2)
  })
})
