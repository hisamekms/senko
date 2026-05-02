import { createMiddleware, createStart } from '@tanstack/react-start'

import {
  REQUEST_NONCE_GLOBAL_KEY,
  buildSecurityHeaders,
  generateNonce,
} from '#/utils/security/csp'

type RunWithNonce = <T>(nonce: string, fn: () => T | Promise<T>) => Promise<T>

let runWithNonce: RunWithNonce = async (_nonce, fn) => fn() as Promise<never>

if (import.meta.env.SSR) {
  const { AsyncLocalStorage } = await import('node:async_hooks')
  const store = new AsyncLocalStorage<string>()
  runWithNonce = (nonce, fn) =>
    store.run(nonce, () => Promise.resolve(fn())) as Promise<never>
  ;(globalThis as { [k: symbol]: unknown })[REQUEST_NONCE_GLOBAL_KEY] = ():
    | string
    | undefined => store.getStore()
}

const securityHeadersMiddleware = createMiddleware({ type: 'request' }).server(
  async ({ next }) => {
    const nonce = generateNonce()
    const result = await runWithNonce(nonce, () => next())
    const isDev = process.env.NODE_ENV !== 'production'
    const headers = buildSecurityHeaders({ nonce, isDev })
    for (const [name, value] of Object.entries(headers)) {
      result.response.headers.set(name, value)
    }
    return result
  },
)

export const startInstance = createStart(() => ({
  requestMiddleware: [securityHeadersMiddleware],
}))
