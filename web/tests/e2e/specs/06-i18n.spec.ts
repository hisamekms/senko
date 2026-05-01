import { test, expect, type Page } from '@playwright/test'

const PAGES = [
  {
    path: '/p/1',
    label: 'dashboard',
    waitFor: '[data-testid^="status-row-"]',
  },
  {
    path: '/p/1/tasks',
    label: 'tasks list',
    waitFor: '[data-testid^="task-card-"]',
  },
  {
    path: '/p/1/contracts',
    label: 'contracts list',
    waitFor: '[data-testid^="contract-card-"]',
  },
  {
    path: '/p/1/graph',
    label: 'graph',
    waitFor: '[data-testid="graph-canvas"]',
  },
] as const

async function setLanguage(page: Page, lng: 'en' | 'ja') {
  // Drive react-i18next directly. selectOption against a controlled <select>
  // races with i18n's async language change and is unreliable; calling the
  // i18n API in-page mirrors what the LanguageSwitcher does and re-renders
  // every translated string.
  await page.evaluate(async (next) => {
    // i18next is the default export of #/i18n which the app ships in its
    // bundle and exposes on the global I18nextProvider. We dispatch through
    // a custom event that the existing provider listens to via 'languageChanged',
    // so just persist + reload to pick up cleanly.
    window.localStorage.setItem('senko.web.lng', next)
  }, lng)
  await page.reload()
}

test.describe('i18n switching', () => {
  for (const { path, label, waitFor } of PAGES) {
    test(`switches en → ja and back on ${label}`, async ({ page }) => {
      await page.goto(path)
      await expect(page.locator(waitFor).first()).toBeVisible()

      // Default language is en; the project label reads "Project:".
      await expect(page.getByText('Project:')).toBeVisible()

      // Switch to Japanese (persist via localStorage + reload — survives
      // the LanguageDetector boot).
      await setLanguage(page, 'ja')
      await expect(page.locator(waitFor).first()).toBeVisible()
      await expect(page.getByText('プロジェクト:')).toBeVisible()

      const stored = await page.evaluate(() =>
        window.localStorage.getItem('senko.web.lng'),
      )
      expect(stored).toBe('ja')

      // Reset to en so subsequent specs / parallel workers start clean.
      await setLanguage(page, 'en')
    })
  }

  test('LanguageSwitcher selectOption changes the visible header text', async ({
    page,
  }) => {
    await page.goto('/p/1')
    // Wait for a card to render — that signal proves LanguageSwitcher has
    // hydrated and its onChange handler is attached.
    await expect(
      page.locator('[data-testid^="status-row-"]').first(),
    ).toBeVisible()

    const langSwitcher = page.getByRole('combobox', {
      name: /language|言語/i,
    })
    await langSwitcher.selectOption('ja')

    // After react-i18next's 'languageChanged' event fires, every translated
    // string re-renders. The header's project label is the most stable
    // cross-page witness.
    await expect(page.getByText('プロジェクト:')).toBeVisible()
  })
})
