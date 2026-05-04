import { useTranslation } from 'react-i18next'

import { DashboardCard } from '#/components/dashboard/DashboardCard'
import { fetchReadyTasks } from '#/components/dashboard/fetchers'
import { useApi } from '#/hooks/useApi'
import { type components } from '#/api'
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

const moreLinkStyle = css({
  display: 'block',
  marginTop: '2',
  paddingX: '2',
  paddingY: '1',
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  textDecoration: 'none',
  textAlign: 'right',
  _hover: { opacity: '1', textDecoration: 'underline' },
})

interface ReadyTasksCardProps {
  projectId: number
}

export function ReadyTasksCard({ projectId }: ReadyTasksCardProps) {
  const { t } = useTranslation()

  const { data, error, loading, reload } = useApi<Task[]>(
    () => fetchReadyTasks(projectId),
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
      <a
        href={`/p/${projectId}/tasks?ready=true`}
        className={moreLinkStyle}
        data-testid="ready-more-link"
      >
        {t('dashboard.ready.more')}
      </a>
    </DashboardCard>
  )
}
