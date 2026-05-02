import { getToken } from '@auth/core/jwt'
import { createFileRoute } from '@tanstack/react-router'

const HOP_BY_HOP_REQUEST_HEADERS = ['host', 'connection', 'content-length']
const HOP_BY_HOP_RESPONSE_HEADERS = [
  'connection',
  'content-encoding',
  'transfer-encoding',
]

async function proxy({ request }: { request: Request }): Promise<Response> {
  const devBypass = process.env.WEB_DEV_AUTH_BYPASS === 'true'

  let accessToken: string | undefined
  if (!devBypass) {
    const secureCookie = new URL(request.url).protocol === 'https:'
    const token = await getToken({
      req: request,
      secret: process.env.AUTH_SECRET,
      secureCookie,
    })
    accessToken = token?.access_token

    if (!accessToken) {
      return new Response(JSON.stringify({ error: 'unauthorized' }), {
        status: 401,
        headers: { 'content-type': 'application/json' },
      })
    }
  }

  const baseUrl = process.env.SENKO_API_BASE_URL
  if (!baseUrl) {
    return new Response(
      JSON.stringify({ error: 'SENKO_API_BASE_URL is not configured' }),
      { status: 500, headers: { 'content-type': 'application/json' } },
    )
  }

  const url = new URL(request.url)
  const upstreamPath = url.pathname.replace(/^\/api\/senko/, '')
  const upstreamUrl = baseUrl.replace(/\/$/, '') + upstreamPath + url.search

  const headers = new Headers(request.headers)
  for (const name of HOP_BY_HOP_REQUEST_HEADERS) headers.delete(name)
  headers.delete('cookie')
  if (accessToken) {
    headers.set('authorization', `Bearer ${accessToken}`)
  } else {
    headers.delete('authorization')
  }

  const hasBody = !['GET', 'HEAD'].includes(request.method)
  const init: RequestInit & { duplex?: 'half' } = {
    method: request.method,
    headers,
    redirect: 'manual',
  }
  if (hasBody) {
    init.body = request.body
    init.duplex = 'half'
  }

  const upstream = await fetch(upstreamUrl, init)
  const responseHeaders = new Headers(upstream.headers)
  for (const name of HOP_BY_HOP_RESPONSE_HEADERS) responseHeaders.delete(name)

  return new Response(upstream.body, {
    status: upstream.status,
    statusText: upstream.statusText,
    headers: responseHeaders,
  })
}

export const Route = createFileRoute('/api/senko/$')({
  server: {
    handlers: {
      GET: proxy,
      POST: proxy,
      PUT: proxy,
      PATCH: proxy,
      DELETE: proxy,
      OPTIONS: proxy,
      HEAD: proxy,
    },
  },
})
