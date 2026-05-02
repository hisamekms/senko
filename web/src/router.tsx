import './utils/assert-prod'

import { createRouter as createTanStackRouter } from '@tanstack/react-router'
import { routeTree } from './routeTree.gen'
import { readCurrentRequestNonce } from '#/utils/security/csp'

export function getRouter() {
  const nonce = readCurrentRequestNonce()
  const router = createTanStackRouter({
    routeTree,
    scrollRestoration: true,
    defaultPreload: 'intent',
    defaultPreloadStaleTime: 0,
    context: { session: null },
    ...(nonce ? { ssr: { nonce } } : {}),
  })

  return router
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof getRouter>
  }
}
