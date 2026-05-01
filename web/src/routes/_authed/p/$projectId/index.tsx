import { createFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { ContractsCard } from '#/components/dashboard/ContractsCard'
import { ReadyTasksCard } from '#/components/dashboard/ReadyTasksCard'
import { RecentTasksCard } from '#/components/dashboard/RecentTasksCard'
import { StatusSummaryCard } from '#/components/dashboard/StatusSummaryCard'
import { css } from '../../../../../styled-system/css'

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '4',
})

const titleStyle = css({
  fontSize: '2xl',
  fontWeight: 'bold',
  color: 'fg',
})

const gridStyle = css({
  display: 'grid',
  gridTemplateColumns: { base: '1fr', md: 'repeat(2, minmax(0, 1fr))' },
  gap: '4',
})

export const Route = createFileRoute('/_authed/p/$projectId/')({
  component: Dashboard,
})

function Dashboard() {
  const { t } = useTranslation()
  const { projectId } = Route.useParams()
  const numericId = Number(projectId)

  if (!Number.isFinite(numericId)) {
    return null
  }

  return (
    <div className={containerStyle}>
      <h1 className={titleStyle}>{t('dashboard.title')}</h1>
      <div className={gridStyle}>
        <StatusSummaryCard projectId={numericId} />
        <RecentTasksCard projectId={numericId} />
        <ReadyTasksCard projectId={numericId} />
        <ContractsCard projectId={numericId} />
      </div>
    </div>
  )
}
