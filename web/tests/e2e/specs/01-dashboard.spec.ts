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

  // Task #430 — dashboard cards must be single-shot, not paginated loops.
  test('cards make at most one tasks/contracts request each on load', async ({
    page,
  }) => {
    const taskRequests: string[] = []
    const contractRequests: string[] = []
    page.on('request', (req) => {
      const url = new URL(req.url())
      if (url.pathname === '/api/senko/api/v1/projects/1/tasks') {
        taskRequests.push(url.search)
      } else if (url.pathname === '/api/senko/api/v1/projects/1/contracts') {
        contractRequests.push(url.search)
      }
    })

    await page.goto('/p/1')
    // Wait for cards to settle (Recent always renders > 0 from the seed).
    await expect(
      page.locator('[data-testid^="recent-task-"]').first(),
    ).toBeVisible()
    // Give the runtime a beat in case any straggler pagination loop fires.
    await page.waitForTimeout(500)

    // Recent + Ready both hit /tasks but with distinct queries, so up to 2
    // calls is the *expected* steady state. The bug we're guarding against
    // is collectAll() emitting 5+ calls per card.
    expect(
      taskRequests.length,
      `tasks endpoint hit ${taskRequests.length} times: ${JSON.stringify(taskRequests)}`,
    ).toBeLessThanOrEqual(2)
    expect(
      contractRequests.length,
      `contracts endpoint hit ${contractRequests.length} times: ${JSON.stringify(contractRequests)}`,
    ).toBeLessThanOrEqual(1)

    // Sanity: confirm the new query shape is in use (proves we're not on the
    // legacy collectAll path).
    expect(taskRequests.some((q) => q.includes('order_by=updated_at'))).toBe(
      true,
    )
    expect(taskRequests.some((q) => q.includes('order_by=priority'))).toBe(
      true,
    )
    expect(contractRequests.some((q) => q.includes('order_by=updated_at'))).toBe(
      true,
    )
  })

  // Per task #433, every dashboard card carries a permanent "See all" link.
  // The card-level test covers a "many items" mock and pins the link target.
  test('Ready/Contracts more link points at the correct destination', async ({
    page,
  }) => {
    // Mock plenty of items per card so the link's destination is what we're
    // really asserting (count is decoupled from link visibility now).
    const fakeTask = (i: number) => ({
      id: i + 100,
      project_id: 1,
      title: `mock task ${i}`,
      status: 'todo',
      priority: 'P2',
      tags: [],
      definition_of_done: [],
      in_scope: [],
      out_of_scope: [],
      dependencies: [],
      assignee_user_id: null,
      contract_id: null,
      branch: null,
      pr_url: null,
      metadata: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      started_at: null,
      completed_at: null,
      canceled_at: null,
      cancel_reason: null,
      assignee_session_id: null,
      background: null,
      description: null,
      plan: null,
    })
    const fakeContract = (i: number) => ({
      id: i + 200,
      project_id: 1,
      title: `mock contract ${i}`,
      description: null,
      definition_of_done: [],
      tags: [],
      metadata: null,
      is_completed: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    })

    await page.route(
      (url) =>
        url.pathname === '/api/senko/api/v1/projects/1/tasks' &&
        url.searchParams.get('ready') === 'true',
      async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            items: Array.from({ length: 20 }, (_, i) => fakeTask(i)),
            next_cursor: 'mock-next-cursor',
          }),
        })
      },
    )
    await page.route(
      (url) => url.pathname === '/api/senko/api/v1/projects/1/contracts',
      async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            items: Array.from({ length: 20 }, (_, i) => fakeContract(i)),
            next_cursor: 'mock-next-cursor',
          }),
        })
      },
    )

    await page.goto('/p/1')
    await expect(page.getByTestId('ready-more-link')).toBeVisible()
    await expect(page.getByTestId('contracts-more-link')).toBeVisible()
    // Sanity: links go to the right destinations.
    await expect(page.getByTestId('ready-more-link')).toHaveAttribute(
      'href',
      '/p/1/tasks?ready=true',
    )
    await expect(page.getByTestId('contracts-more-link')).toHaveAttribute(
      'href',
      '/p/1/contracts',
    )
  })

  test('Ready/Contracts more link is permanent regardless of count', async ({
    page,
  }) => {
    // Per task #433, every dashboard card carries a permanent "See all" link
    // — the prior "only when truncated" gating is gone. Mock 5 items (below
    // any earlier limit) and assert both more links still render.
    const fakeTask = (i: number) => ({
      id: i + 100,
      project_id: 1,
      title: `mock task ${i}`,
      status: 'todo',
      priority: 'P2',
      tags: [],
      definition_of_done: [],
      in_scope: [],
      out_of_scope: [],
      dependencies: [],
      assignee_user_id: null,
      contract_id: null,
      branch: null,
      pr_url: null,
      metadata: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      started_at: null,
      completed_at: null,
      canceled_at: null,
      cancel_reason: null,
      assignee_session_id: null,
      background: null,
      description: null,
      plan: null,
    })
    const fakeContract = (i: number) => ({
      id: i + 200,
      project_id: 1,
      title: `mock contract ${i}`,
      description: null,
      definition_of_done: [],
      tags: [],
      metadata: null,
      is_completed: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    })

    await page.route(
      (url) =>
        url.pathname === '/api/senko/api/v1/projects/1/tasks' &&
        url.searchParams.get('ready') === 'true',
      async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            items: Array.from({ length: 5 }, (_, i) => fakeTask(i)),
            next_cursor: null,
          }),
        })
      },
    )
    await page.route(
      (url) => url.pathname === '/api/senko/api/v1/projects/1/contracts',
      async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            items: Array.from({ length: 5 }, (_, i) => fakeContract(i)),
            next_cursor: null,
          }),
        })
      },
    )

    await page.goto('/p/1')
    // Wait for ready card to render so the test isn't asserting against
    // pre-render absence.
    await expect(
      page.locator('[data-testid^="ready-task-"]').first(),
    ).toBeVisible()
    await expect(page.getByTestId('ready-more-link')).toBeVisible()
    await expect(page.getByTestId('contracts-more-link')).toBeVisible()
  })
})
