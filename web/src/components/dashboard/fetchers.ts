import { apiClient, type ApiClient, type components } from '#/api'

type Task = components['schemas']['TaskResponse']
type Contract = components['schemas']['ContractResponse']

const RECENT_LIMIT = 5
const READY_LIMIT = 5
const CONTRACTS_LIMIT = 5

/**
 * Fetch the most recently updated tasks for the dashboard "Recent" card.
 * Single-shot request — no client-side pagination, no client-side sort.
 */
export async function fetchRecentTasks(
  projectId: number,
  client: ApiClient = apiClient,
): Promise<Task[]> {
  const { data, error } = await client.GET(
    '/api/v1/projects/{project_id}/tasks',
    {
      params: {
        path: { project_id: projectId },
        query: { order_by: 'updated_at', order: 'desc', limit: RECENT_LIMIT },
      },
    },
  )
  if (error || !data) throw new Error('Failed to load recent tasks')
  return data.items
}

/**
 * Fetch ready tasks (status=todo with all deps completed) ordered by priority,
 * P0 first. Single-shot request capped at 5 items — the dashboard summarises;
 * the full list lives at `/p/{projectId}/tasks?ready=true`.
 */
export async function fetchReadyTasks(
  projectId: number,
  client: ApiClient = apiClient,
): Promise<Task[]> {
  const { data, error } = await client.GET(
    '/api/v1/projects/{project_id}/tasks',
    {
      params: {
        path: { project_id: projectId },
        query: {
          ready: true,
          order_by: 'priority',
          order: 'asc',
          limit: READY_LIMIT,
        },
      },
    },
  )
  if (error || !data) throw new Error('Failed to load ready tasks')
  return data.items
}

/**
 * Fetch the most recently updated contracts. Capped at 5 items; the full list
 * lives at `/p/{projectId}/contracts`.
 */
export async function fetchContracts(
  projectId: number,
  client: ApiClient = apiClient,
): Promise<Contract[]> {
  const { data, error } = await client.GET(
    '/api/v1/projects/{project_id}/contracts',
    {
      params: {
        path: { project_id: projectId },
        query: {
          order_by: 'updated_at',
          order: 'desc',
          limit: CONTRACTS_LIMIT,
        },
      },
    },
  )
  if (error || !data) throw new Error('Failed to load contracts')
  return data.items
}
