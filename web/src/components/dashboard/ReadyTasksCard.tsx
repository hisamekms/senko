import { useTranslation } from 'react-i18next'

import { DashboardCard } from '#/components/dashboard/DashboardCard'
import { useApi } from '#/hooks/useApi'
import { apiClient, collectAll, type components } from '#/api'
import { css } from '../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
})

const rowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
  paddingX: '2',
  paddingY: '2',
  borderRadius: 'sm',
  color: 'fg',
  textDecoration: 'none',
  _hover: { backgroundColor: 'bg' },
})

const taskTitleStyle = css({
  fontSize: 'sm',
  fontWeight: 'medium',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

const priorityStyle = css({
  fontSize: 'xs',
  color: 'accent',
  fontWeight: 'semibold',
})

interface ReadyTasksCardProps {
  projectId: number
}

export function ReadyTasksCard({ projectId }: ReadyTasksCardProps) {
  const { t } = useTranslation()

  const { data, error, loading, reload } = useApi<Task[]>(
    () =>
      collectAll<Task>(async (cursor) => {
        const { data, error } = await apiClient.GET(
          '/api/v1/projects/{project_id}/tasks',
          {
            params: {
              path: { project_id: projectId },
              query: { ready: true, ...(cursor ? { after: cursor } : {}) },
            },
          },
        )
        if (error || !data) throw new Error('Failed to load ready tasks')
        return { items: data.items, next_cursor: data.next_cursor ?? null }
      }),
    [projectId],
  )

  const tasks = data ?? []

  return (
    <DashboardCard
      title={t('dashboard.ready.title')}
      loading={loading}
      error={error}
      empty={!loading && !error && tasks.length === 0}
      emptyMessage={t('dashboard.ready.empty')}
      onRetry={reload}
      headerRight={!loading && !error ? String(tasks.length) : undefined}
    >
      <ul className={listStyle}>
        {tasks.map((task) => (
          <li key={task.id}>
            <a
              href={`/p/${projectId}/tasks/${task.id}`}
              className={rowStyle}
              data-testid={`ready-task-${task.id}`}
            >
              <span className={taskTitleStyle}>
                #{task.id} {task.title}
              </span>
              <span className={priorityStyle}>{task.priority}</span>
            </a>
          </li>
        ))}
      </ul>
    </DashboardCard>
  )
}
