import createClient, { type Client, type Middleware } from 'openapi-fetch'

import type { paths } from './types.gen'

export type ApiClient = Client<paths>

const DEFAULT_BASE_URL = '/api/senko'
const LOGIN_PATH = '/login'

export interface CreateApiClientOptions {
  baseUrl?: string
  fetch?: typeof fetch
  onUnauthorized?: () => void
}

const defaultOnUnauthorized = (): void => {
  if (typeof window !== 'undefined') {
    window.location.href = LOGIN_PATH
  }
}

export function createApiClient(
  options: CreateApiClientOptions = {},
): ApiClient {
  const onUnauthorized = options.onUnauthorized ?? defaultOnUnauthorized

  const client = createClient<paths>({
    baseUrl: options.baseUrl ?? DEFAULT_BASE_URL,
    fetch: options.fetch,
  })

  const unauthorizedRedirect: Middleware = {
    async onResponse({ response }) {
      if (response.status === 401) {
        onUnauthorized()
      }
      return response
    },
  }

  client.use(unauthorizedRedirect)

  return client
}

export const apiClient: ApiClient = createApiClient()
