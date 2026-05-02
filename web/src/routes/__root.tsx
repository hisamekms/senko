import {
  HeadContent,
  Outlet,
  Scripts,
  createRootRouteWithContext,
} from '@tanstack/react-router'
import { TanStackRouterDevtoolsPanel } from '@tanstack/react-router-devtools'
import { TanStackDevtools } from '@tanstack/react-devtools'
import { createServerFn } from '@tanstack/react-start'
import { getRequest } from '@tanstack/react-start/server'
import { I18nextProvider } from 'react-i18next'
import { getSession, type AuthSession } from 'start-authjs'

import i18n from '#/i18n'
import { getAuthConfig } from '#/utils/auth'
import appCss from '../styles.css?url'

export interface RouterContext {
  session: AuthSession | null
}

const fetchSession = createServerFn({ method: 'GET' }).handler(async () => {
  if (
    process.env.NODE_ENV !== 'production' &&
    process.env.WEB_DEV_AUTH_BYPASS === 'true'
  ) {
    const expires = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString()
    return {
      user: { name: 'dev-user', email: 'dev@localhost' },
      expires,
    } satisfies AuthSession
  }
  const request = getRequest()
  return await getSession(request, await getAuthConfig())
})

const themeBootstrap = `(function(){try{var k='senko.web.theme';var t=localStorage.getItem(k);if(t!=='light'&&t!=='dark'){t=window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';}document.documentElement.dataset.theme=t;}catch(e){document.documentElement.dataset.theme='light';}})();`

export const Route = createRootRouteWithContext<RouterContext>()({
  beforeLoad: async () => {
    const session = await fetchSession()
    return { session }
  },
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'senko' },
    ],
    links: [{ rel: 'stylesheet', href: appCss }],
    scripts: [{ children: themeBootstrap }],
  }),
  shellComponent: RootDocument,
})

function RootDocument({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" data-theme="light">
      <head>
        <HeadContent />
      </head>
      <body>
        <I18nextProvider i18n={i18n}>
          {children ?? <Outlet />}
        </I18nextProvider>
        <TanStackDevtools
          config={{ position: 'bottom-right' }}
          plugins={[
            {
              name: 'Tanstack Router',
              render: <TanStackRouterDevtoolsPanel />,
            },
          ]}
        />
        <Scripts />
      </body>
    </html>
  )
}
