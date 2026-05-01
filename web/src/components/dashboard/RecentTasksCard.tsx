import { useTranslation } from 'react-i18next'

import { DashboardCard } from '#/components/dashboard/DashboardCard'
import { useApi } from '#/hooks/useApi'
import { apiClient, collectAll, type components } from '#/api'
import { css } from '../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

const RECENT_LIMIT = 10

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
})

const rowStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '0.5',
  paddingX: '2',
  paddingY: '2',
  borderRadius: 'sm',
  color: 'fg',
  textDecoration: 'none',
  _hover: { backgroundColor: 'bg' },
})

const titleRowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
})

const taskTitleStyle = css({
  fontSize: 'sm',
  fontWeight: 'medium',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

const metaStyle = css({
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
  whiteSpace: 'nowrap',
})

const statusStyle = css({
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
})

interface RecentTasksCardProps {
  projectId: number
}

export function RecentTasksCard({ projectId }: RecentTasksCardProps) {
  const { t } = useTranslation()

  const { data, error, loading, reload } = useApi<Task[]>(
    () =>
      collectAll<Task>(async (cursor) => {
        const { data, error } = await apiClient.GET(
          '/api/v1/projects/{project_id}/tasks',
          {
            params: {
              path: { project_id: projectId },
              query: cursor ? { after: cursor } : {},
            },
          },
        )
        if (error || !data) throw new Error('Failed to load tasks')
        return { items: data.items, next_cursor: data.next_cursor ?? null }
      }),
    [projectId],
  )

  const tasks = (data ?? [])
    .slice()
    .sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1))
    .slice(0, RECENT_LIMIT)

  return (
    <DashboardCard
      title={t('dashboard.recent.title')}
      loading={loading}
      error={error}
      empty={!loading && !error && tasks.length === 0}
      emptyMessage={t('dashboard.recent.empty')}
      onRetry={reload}
    >
      <ul className={listStyle}>
        {tasks.map((task) => (
          <li key={task.id}>
            <a
              href={`/p/${projectId}/tasks/${task.id}`}
              className={rowStyle}
              data-testid={`recent-task-${task.id}`}
            >
              <div className={titleRowStyle}>
                <span className={taskTitleStyle}>
                  #{task.id} {task.title}
                </span>
                <span className={metaStyle}>{formatDate(task.updated_at)}</span>
              </div>
              <span className={statusStyle}>
                {t(`dashboard.status.${task.status}`, {
                  defaultValue: task.status,
                })}
              </span>
            </a>
          </li>
        ))}
      </ul>
    </DashboardCard>
  )
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toISOString().slice(0, 10)
  } catch {
    return iso
  }
}
