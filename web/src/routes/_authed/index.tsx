import { createFileRoute, redirect } from '@tanstack/react-router'
import { createServerFn } from '@tanstack/react-start'
import { getRequest } from '@tanstack/react-start/server'
import { useTranslation } from 'react-i18next'

import { AppHeader } from '#/components/AppHeader'
import { css } from '../../../styled-system/css'

interface ProjectSummary {
  id: number
  name: string
}

const fetchAccessibleProjects = createServerFn({ method: 'GET' }).handler(
  async (): Promise<ProjectSummary[]> => {
    const request = getRequest()
    const url = new URL('/api/senko/api/v1/projects', request.url)
    const headers = new Headers()
    const cookie = request.headers.get('cookie')
    if (cookie) headers.set('cookie', cookie)
    const response = await fetch(url, { headers })
    if (!response.ok) return []
    const body = (await response.json()) as { items: ProjectSummary[] }
    return body.items ?? []
  },
)

const pageStyle = css({
  minHeight: '100vh',
  display: 'flex',
  flexDirection: 'column',
  backgroundColor: 'bg',
})

const mainStyle = css({
  flex: '1',
  paddingX: '6',
  paddingY: '8',
  display: 'flex',
  flexDirection: 'column',
  gap: '4',
  maxWidth: '720px',
  marginX: 'auto',
  width: '100%',
})

const headingStyle = css({
  fontSize: '2xl',
  fontWeight: 'bold',
  color: 'fg',
})

const messageStyle = css({
  fontSize: 'md',
  color: 'fg',
  opacity: '0.8',
})

export const Route = createFileRoute('/_authed/')({
  beforeLoad: async () => {
    const projects = await fetchAccessibleProjects()
    if (projects.length > 0) {
      throw redirect({
        to: '/p/$projectId',
        params: { projectId: String(projects[0].id) },
      })
    }
    return { projects }
  },
  component: NoProjects,
})

function NoProjects() {
  const { t } = useTranslation()
  const { session } = Route.useRouteContext()
  const userLabel = session?.user?.name ?? session?.user?.email ?? null

  return (
    <div className={pageStyle}>
      <AppHeader currentProjectId={null} userLabel={userLabel} />
      <main className={mainStyle}>
        <h1 className={headingStyle}>{t('dashboard.title')}</h1>
        <p className={messageStyle}>{t('dashboard.empty.noProjects')}</p>
        <p className={messageStyle}>{t('dashboard.empty.noProjectsHint')}</p>
      </main>
    </div>
  )
}
