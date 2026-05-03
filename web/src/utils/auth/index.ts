import type { OIDCConfig } from '@auth/core/providers'
import type { Profile } from '@auth/core/types'
import type { StartAuthJSConfig } from 'start-authjs'

import { refreshAccessToken } from '#/utils/auth/refresh'
import { resolveSecret } from '#/utils/secrets/resolve'

declare module '@auth/core/jwt' {
  interface JWT {
    access_token?: string
    id_token?: string
    refresh_token?: string
    expires_at?: number
    error?: 'RefreshAccessTokenError'
  }
}

declare module '@auth/core/types' {
  interface Session {
    error?: 'RefreshAccessTokenError'
  }
}

declare module 'start-authjs' {
  interface AuthSession {
    error?: 'RefreshAccessTokenError'
  }
}

const REFRESH_LEEWAY_MS = 60_000

export async function getAuthConfig(): Promise<StartAuthJSConfig> {
  const [authSecret, oidcClientSecret] = await Promise.all([
    resolveSecret('AUTH_SECRET'),
    resolveSecret('AUTH_OIDC_CLIENT_SECRET'),
  ])

  const oidcProvider: OIDCConfig<Profile> = {
    id: 'oidc',
    name: 'OIDC',
    type: 'oidc',
    issuer: process.env.AUTH_OIDC_ISSUER,
    clientId: process.env.AUTH_OIDC_CLIENT_ID,
    clientSecret: oidcClientSecret,
    checks: ['pkce', 'state', 'nonce'],
    authorization: {
      params: {
        scope:
          process.env.AUTH_OIDC_SCOPES ?? 'openid profile email offline_access',
      },
    },
  }

  return {
    secret: authSecret,
    providers: [oidcProvider],
    callbacks: {
      async jwt({ token, account }) {
        if (account) {
          const expiresAtMs =
            typeof account.expires_at === 'number'
              ? account.expires_at * 1000
              : typeof account.expires_in === 'number'
                ? Date.now() + account.expires_in * 1000
                : undefined
          return {
            ...token,
            access_token:
              typeof account.access_token === 'string'
                ? account.access_token
                : undefined,
            id_token:
              typeof account.id_token === 'string'
                ? account.id_token
                : undefined,
            refresh_token:
              typeof account.refresh_token === 'string'
                ? account.refresh_token
                : undefined,
            expires_at: expiresAtMs,
            error: undefined,
          }
        }

        if (!token.expires_at || !token.refresh_token) {
          return token
        }

        if (Date.now() < token.expires_at - REFRESH_LEEWAY_MS) {
          return token
        }

        const result = await refreshAccessToken(token.refresh_token)
        if (!result.ok) {
          return { ...token, error: 'RefreshAccessTokenError' as const }
        }
        return {
          ...token,
          access_token: result.access_token,
          expires_at: result.expires_at,
          refresh_token: result.refresh_token ?? token.refresh_token,
          error: undefined,
        }
      },
      async session({ session, token }) {
        if (token.error) {
          session.error = token.error
        }
        return session
      },
    },
  }
}
