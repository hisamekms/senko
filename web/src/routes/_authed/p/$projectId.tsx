import { Outlet, createFileRoute } from '@tanstack/react-router'

import { AppHeader } from '#/components/AppHeader'
import { css } from '../../../../styled-system/css'

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
  width: '100%',
  maxWidth: '1200px',
  marginX: 'auto',
})

export const Route = createFileRoute('/_authed/p/$projectId')({
  component: ProjectLayout,
})

function ProjectLayout() {
  const { projectId } = Route.useParams()
  const { session } = Route.useRouteContext()
  const userLabel = session?.user?.name ?? session?.user?.email ?? null
  const numericId = Number(projectId)

  return (
    <div className={pageStyle}>
      <AppHeader
        currentProjectId={Number.isFinite(numericId) ? numericId : null}
        userLabel={userLabel}
      />
      <main className={mainStyle}>
        <Outlet />
      </main>
    </div>
  )
}
