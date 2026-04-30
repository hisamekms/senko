import type { OIDCConfig } from '@auth/core/providers'
import type { Profile } from '@auth/core/types'
import type { StartAuthJSConfig } from 'start-authjs'

declare module '@auth/core/jwt' {
  interface JWT {
    access_token?: string
  }
}

declare module '@auth/core/types' {
  interface Session {
    access_token?: string
  }
}

declare module 'start-authjs' {
  interface AuthSession {
    access_token?: string
  }
}

const oidcProvider: OIDCConfig<Profile> = {
  id: 'oidc',
  name: 'OIDC',
  type: 'oidc',
  issuer: process.env.AUTH_OIDC_ISSUER,
  clientId: process.env.AUTH_OIDC_CLIENT_ID,
  clientSecret: process.env.AUTH_OIDC_CLIENT_SECRET,
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
      return token
    },
    async session({ session, token }) {
      if (token.access_token) {
        session.access_token = token.access_token
      }
      return session
    },
  },
}
