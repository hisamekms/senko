import type { OIDCConfig } from '@auth/core/providers'
import type { Profile } from '@auth/core/types'
import type { StartAuthJSConfig } from 'start-authjs'

declare module '@auth/core/jwt' {
  interface JWT {
    access_token?: string
    id_token?: string
  }
}

const oidcProvider: OIDCConfig<Profile> = {
  id: 'oidc',
  name: 'OIDC',
  type: 'oidc',
  issuer: process.env.AUTH_OIDC_ISSUER,
  clientId: process.env.AUTH_OIDC_CLIENT_ID,
  clientSecret: process.env.AUTH_OIDC_CLIENT_SECRET,
  checks: ['pkce', 'state', 'nonce'],
  authorization: {
    params: {
      scope: 'openid profile email',
    },
  },
}

export const authConfig: StartAuthJSConfig = {
  secret: process.env.AUTH_SECRET,
  providers: [oidcProvider],
  callbacks: {
    async jwt({ token, account }) {
      if (account?.access_token) {
        token.access_token = account.access_token
      }
      if (account?.id_token) {
        token.id_token = account.id_token
      }
      return token
    },
  },
}
