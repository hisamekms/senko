import { getTokenEndpoint as defaultGetTokenEndpoint } from '#/utils/security/oidc-discovery'
import { resolveSecret } from '#/utils/secrets/resolve'

export type RefreshResult =
  | {
      ok: true
      access_token: string
      expires_at: number
      refresh_token?: string
    }
  | { ok: false }

interface RefreshDeps {
  fetchImpl?: typeof fetch
  getTokenEndpoint?: () => Promise<string | null>
  clientId?: string
  clientSecret?: string
  scope?: string
}

interface TokenResponse {
  access_token?: unknown
  expires_in?: unknown
  refresh_token?: unknown
}

export async function refreshAccessToken(
  refreshToken: string,
  deps: RefreshDeps = {},
): Promise<RefreshResult> {
  const fetchImpl = deps.fetchImpl ?? fetch
  const getEndpoint = deps.getTokenEndpoint ?? defaultGetTokenEndpoint
  const clientId = deps.clientId ?? process.env.AUTH_OIDC_CLIENT_ID
  const clientSecret =
    deps.clientSecret ?? (await resolveSecret('AUTH_OIDC_CLIENT_SECRET'))
  const scope =
    deps.scope ??
    process.env.AUTH_OIDC_SCOPES ??
    'openid profile email offline_access'

  if (!clientId || !clientSecret) return { ok: false }

  const endpoint = await getEndpoint()
  if (!endpoint) return { ok: false }

  const body = new URLSearchParams({
    grant_type: 'refresh_token',
    refresh_token: refreshToken,
    scope,
  })

  const basic = Buffer.from(`${clientId}:${clientSecret}`).toString('base64')

  let res: Response
  try {
    res = await fetchImpl(endpoint, {
      method: 'POST',
      headers: {
        'content-type': 'application/x-www-form-urlencoded',
        accept: 'application/json',
        authorization: `Basic ${basic}`,
      },
      body,
      signal: AbortSignal.timeout(10_000),
    })
  } catch {
    return { ok: false }
  }

  if (!res.ok) return { ok: false }

  let json: TokenResponse
  try {
    json = (await res.json()) as TokenResponse
  } catch {
    return { ok: false }
  }

  if (typeof json.access_token !== 'string') return { ok: false }
  if (typeof json.expires_in !== 'number' || !Number.isFinite(json.expires_in)) {
    return { ok: false }
  }

  const refreshed: RefreshResult = {
    ok: true,
    access_token: json.access_token,
    expires_at: Date.now() + json.expires_in * 1000,
  }
  if (typeof json.refresh_token === 'string' && json.refresh_token.length > 0) {
    refreshed.refresh_token = json.refresh_token
  }
  return refreshed
}
