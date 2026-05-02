import { getToken } from '@auth/core/jwt'
import { createFileRoute } from '@tanstack/react-router'
import { StartAuthJS } from 'start-authjs'

import { getAuthConfig } from '#/utils/auth'
import { getEndSessionEndpoint } from '#/utils/security/oidc-discovery'
import { resolveSecret } from '#/utils/secrets/resolve'

const { GET, POST } = StartAuthJS(() => getAuthConfig())

function isSignoutPath(pathname: string): boolean {
  return /\/signout(?:\/[^/]+)?$/.test(pathname)
}

async function rpInitiatedSignout(request: Request): Promise<Response> {
  const url = new URL(request.url)
  const secureCookie = url.protocol === 'https:'

  const token = await getToken({
    req: request,
    secret: await resolveSecret('AUTH_SECRET'),
    secureCookie,
  })
  const idToken = token?.id_token

  const res = await POST({ request, response: new Response() })

  if (!idToken) return res
  if (res.status < 300 || res.status >= 400) return res
  if (!res.headers.get('Location')) return res

  const endSession = await getEndSessionEndpoint()
  if (!endSession) return res

  const target = new URL(endSession)
  target.searchParams.set('id_token_hint', idToken)
  target.searchParams.set('post_logout_redirect_uri', `${url.origin}/login`)

  const headers = new Headers(res.headers)
  headers.set('Location', target.toString())
  return new Response(null, { status: res.status, headers })
}

export const Route = createFileRoute('/api/auth/$')({
  server: {
    handlers: {
      GET: ({ request }) => GET({ request, response: new Response() }),
      POST: ({ request }) => {
        if (isSignoutPath(new URL(request.url).pathname)) {
          return rpInitiatedSignout(request)
        }
        return POST({ request, response: new Response() })
      },
    },
  },
})
