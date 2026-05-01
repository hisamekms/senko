import { useTranslation } from 'react-i18next'

import { DashboardCard } from '#/components/dashboard/DashboardCard'
import { useApi } from '#/hooks/useApi'
import { apiClient } from '#/api'
import { css } from '../../../styled-system/css'

const STATUSES = ['draft', 'todo', 'in_progress', 'completed', 'canceled'] as const
type Status = (typeof STATUSES)[number]

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
})

const rowStyle = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  paddingX: '2',
  paddingY: '2',
  borderRadius: 'sm',
  color: 'fg',
  textDecoration: 'none',
  _hover: { backgroundColor: 'bg' },
})

const labelStyle = css({
  fontSize: 'sm',
})

const countStyle = css({
  fontSize: 'sm',
  fontWeight: 'semibold',
  fontVariantNumeric: 'tabular-nums',
})

interface StatusSummaryCardProps {
  projectId: number
}

export function StatusSummaryCard({ projectId }: StatusSummaryCardProps) {
  const { t } = useTranslation()

  const { data, error, loading, reload } = useApi<Record<string, number>>(
    async () => {
      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/stats',
        { params: { path: { project_id: projectId } } },
      )
      if (error || !data) throw new Error('Failed to load status counts')
      return data as Record<string, number>
    },
    [projectId],
  )

  const counts: Record<Status, number> = {
    draft: 0,
    todo: 0,
    in_progress: 0,
    completed: 0,
    canceled: 0,
  }
  if (data) {
    for (const status of STATUSES) {
      counts[status] = data[status] ?? 0
    }
  }
  const total = STATUSES.reduce((sum, s) => sum + counts[s], 0)

  return (
    <DashboardCard
      title={t('dashboard.status.title')}
      loading={loading}
      error={error}
      empty={!loading && !error && total === 0}
      emptyMessage={t('dashboard.status.empty')}
      onRetry={reload}
    >
      <ul className={listStyle}>
        {STATUSES.map((status) => (
          <li key={status}>
            <a
              href={`/p/${projectId}/tasks?status=${status}`}
              className={rowStyle}
              data-testid={`status-row-${status}`}
            >
              <span className={labelStyle}>{t(`dashboard.status.${status}`)}</span>
              <span className={countStyle}>{counts[status]}</span>
            </a>
          </li>
        ))}
      </ul>
    </DashboardCard>
  )
}
