import { test, expect } from '@playwright/test'

test.describe('Dependency graph', () => {
  test('renders task nodes and navigates to a task on click', async ({
    page,
  }) => {
    await page.goto('/p/1/graph')

    // Canvas is mounted (the React Flow container).
    await expect(page.getByTestId('graph-canvas')).toBeVisible()

    // Wait for at least one node to attach. xyflow uses absolute positioning
    // + transforms; nodes can land outside Playwright's viewport-actionability
    // window even with `force: true`, so dispatch the click event directly.
    const node1 = page.getByTestId('graph-node-1')
    await expect(node1).toBeAttached()
    await node1.dispatchEvent('click')

    // Side panel becomes visible after selection.
    await expect(page.getByTestId('graph-side-panel')).toBeVisible()

    // "Open detail" link routes to /p/1/tasks/1
    const openDetail = page.getByTestId('graph-open-detail')
    await expect(openDetail).toBeVisible()
    await openDetail.click()
    await expect(page).toHaveURL(/\/p\/1\/tasks\/1$/)
  })

  test('graph filters URL through ?status= when applied', async ({ page }) => {
    await page.goto('/p/1/graph')
    await expect(page.getByTestId('graph-canvas')).toBeVisible()

    await page.getByTestId('graph-filter-status-todo').click()
    // GraphFilters encodes `status` as a JSON array (same convention as
    // TaskFilters): `?status=%5B%22todo%22%5D`.
    await expect(page).toHaveURL(/status=(todo|%5B%22todo%22%5D)/)
  })
})
