import { describe, it, expect, vi } from 'vitest'

import { refreshAccessToken } from './refresh'

const FIXED_NOW = 1_700_000_000_000

function makeFetchOk(body: unknown, opts: { invalidJson?: boolean } = {}) {
  return vi.fn(
    async () =>
      new Response(opts.invalidJson ? '<<not json>>' : JSON.stringify(body), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
  )
}

function makeFetchStatus(status: number) {
  return vi.fn(
    async () =>
      new Response(JSON.stringify({ error: 'invalid_grant' }), {
        status,
        headers: { 'content-type': 'application/json' },
      }),
  )
}

const baseDeps = {
  getTokenEndpoint: async () => 'https://idp.example/token',
  clientId: 'client-id-123',
  clientSecret: 'client-secret-xyz',
}

describe('refreshAccessToken', () => {
  it('returns a new access_token on success and computes expires_at from expires_in', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(FIXED_NOW)
    const fetchImpl = makeFetchOk({
      access_token: 'new-access',
      expires_in: 300,
      refresh_token: 'rotated-refresh',
    })

    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })

    expect(result).toEqual({
      ok: true,
      access_token: 'new-access',
      expires_at: FIXED_NOW + 300_000,
      refresh_token: 'rotated-refresh',
    })
    vi.useRealTimers()
  })

  it('omits refresh_token when the IdP does not rotate (caller keeps old one)', async () => {
    const fetchImpl = makeFetchOk({
      access_token: 'new-access',
      expires_in: 300,
    })

    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })

    expect(result.ok).toBe(true)
    if (!result.ok) throw new Error('unreachable')
    expect(result.access_token).toBe('new-access')
    expect(result.refresh_token).toBeUndefined()
  })

  it('returns ok:false on non-2xx response', async () => {
    const fetchImpl = makeFetchStatus(401)
    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })
    expect(result).toEqual({ ok: false })
  })

  it('returns ok:false on invalid JSON', async () => {
    const fetchImpl = makeFetchOk({}, { invalidJson: true })
    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })
    expect(result).toEqual({ ok: false })
  })

  it('returns ok:false when getTokenEndpoint resolves to null', async () => {
    const fetchImpl = vi.fn()
    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      getTokenEndpoint: async () => null,
      fetchImpl,
    })
    expect(result).toEqual({ ok: false })
    expect(fetchImpl).not.toHaveBeenCalled()
  })

  it('returns ok:false when fetch throws (network/timeout)', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new Error('boom')
    })
    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })
    expect(result).toEqual({ ok: false })
  })

  it('returns ok:false when access_token is missing in response', async () => {
    const fetchImpl = makeFetchOk({ expires_in: 300 })
    const result = await refreshAccessToken('old-refresh', {
      ...baseDeps,
      fetchImpl,
    })
    expect(result).toEqual({ ok: false })
  })

  it('uses RefreshDeps.scope when provided (overrides env and default)', async () => {
    const fetchImpl = makeFetchOk({ access_token: 'a', expires_in: 60 })
    const prevEnv = process.env.AUTH_OIDC_SCOPES
    process.env.AUTH_OIDC_SCOPES = 'env-scope-should-be-ignored'
    try {
      await refreshAccessToken('the-refresh', {
        ...baseDeps,
        fetchImpl,
        scope: 'openid profile email',
      })
    } finally {
      if (prevEnv === undefined) {
        delete process.env.AUTH_OIDC_SCOPES
      } else {
        process.env.AUTH_OIDC_SCOPES = prevEnv
      }
    }

    const [, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit]
    const body = init.body as URLSearchParams
    expect(body.get('scope')).toBe('openid profile email')
  })

  it('falls back to AUTH_OIDC_SCOPES env when deps.scope is unset', async () => {
    const fetchImpl = makeFetchOk({ access_token: 'a', expires_in: 60 })
    const prevEnv = process.env.AUTH_OIDC_SCOPES
    process.env.AUTH_OIDC_SCOPES = 'openid profile email'
    try {
      await refreshAccessToken('the-refresh', {
        ...baseDeps,
        fetchImpl,
      })
    } finally {
      if (prevEnv === undefined) {
        delete process.env.AUTH_OIDC_SCOPES
      } else {
        process.env.AUTH_OIDC_SCOPES = prevEnv
      }
    }

    const [, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit]
    const body = init.body as URLSearchParams
    expect(body.get('scope')).toBe('openid profile email')
  })

  it('sends client_secret_basic Authorization header and form-urlencoded body', async () => {
    const fetchImpl = makeFetchOk({ access_token: 'a', expires_in: 60 })
    await refreshAccessToken('the-refresh', {
      ...baseDeps,
      fetchImpl,
    })

    expect(fetchImpl).toHaveBeenCalledTimes(1)
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [
      string,
      RequestInit,
    ]
    expect(url).toBe('https://idp.example/token')
    expect(init.method).toBe('POST')

    const headers = new Headers(init.headers)
    const expected = `Basic ${Buffer.from('client-id-123:client-secret-xyz').toString('base64')}`
    expect(headers.get('authorization')).toBe(expected)
    expect(headers.get('content-type')).toBe(
      'application/x-www-form-urlencoded',
    )

    const body = init.body as URLSearchParams
    expect(body.get('grant_type')).toBe('refresh_token')
    expect(body.get('refresh_token')).toBe('the-refresh')
    expect(body.get('scope')).toContain('offline_access')
  })
})
