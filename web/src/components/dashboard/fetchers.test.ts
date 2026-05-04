import { describe, expect, it, vi } from 'vitest'

import type { ApiClient } from '#/api'

import {
  fetchContracts,
  fetchReadyTasks,
  fetchRecentTasks,
} from './fetchers'

function fakeClient(): { client: ApiClient; get: ReturnType<typeof vi.fn> } {
  const get = vi.fn().mockResolvedValue({
    data: { items: [], next_cursor: null },
    error: undefined,
  })
  // We only exercise the GET method, so the rest of the ApiClient surface can
  // stay unmocked.
  return { client: { GET: get } as unknown as ApiClient, get }
}

describe('dashboard fetchers', () => {
  describe('fetchRecentTasks', () => {
    it('calls GET /tasks exactly once with updated_at desc, limit=5', async () => {
      const { client, get } = fakeClient()
      await fetchRecentTasks(7, client)

      expect(get).toHaveBeenCalledTimes(1)
      expect(get).toHaveBeenCalledWith(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: 7 },
            query: { order_by: 'updated_at', order: 'desc', limit: 5 },
          },
        },
      )
    })

    it('throws on API error', async () => {
      const get = vi.fn().mockResolvedValue({
        data: undefined,
        error: { message: 'boom' },
      })
      const client = { GET: get } as unknown as ApiClient
      await expect(fetchRecentTasks(1, client)).rejects.toThrow(
        /Failed to load recent tasks/,
      )
    })
  })

  describe('fetchReadyTasks', () => {
    it('calls GET /tasks exactly once with ready=true, priority asc, limit=5', async () => {
      const { client, get } = fakeClient()
      await fetchReadyTasks(42, client)

      expect(get).toHaveBeenCalledTimes(1)
      expect(get).toHaveBeenCalledWith(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: 42 },
            query: {
              ready: true,
              order_by: 'priority',
              order: 'asc',
              limit: 5,
            },
          },
        },
      )
    })
  })

  describe('fetchContracts', () => {
    it('calls GET /contracts exactly once with updated_at desc, limit=5', async () => {
      const { client, get } = fakeClient()
      await fetchContracts(99, client)

      expect(get).toHaveBeenCalledTimes(1)
      expect(get).toHaveBeenCalledWith(
        '/api/v1/projects/{project_id}/contracts',
        {
          params: {
            path: { project_id: 99 },
            query: { order_by: 'updated_at', order: 'desc', limit: 5 },
          },
        },
      )
    })
  })
})
