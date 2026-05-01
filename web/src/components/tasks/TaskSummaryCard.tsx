import { useTranslation } from 'react-i18next'

import { TaskStatusBadge } from '#/components/tasks/TaskStatusBadge'
import type { components } from '#/api'
import { css } from '../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

const cardStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
  padding: '3',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
  textDecoration: 'none',
  _hover: { borderColor: 'accent' },
})

const headerRowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '3',
  flexWrap: 'wrap',
})

const titleStyle = css({
  fontSize: 'md',
  fontWeight: 'medium',
  flex: '1',
  minWidth: '0',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

const metaRowStyle = css({
  display: 'flex',
  alignItems: 'center',
  gap: '2',
  flexWrap: 'wrap',
})

const priorityStyle = css({
  fontSize: 'xs',
  fontWeight: 'semibold',
  color: 'accent',
  fontVariantNumeric: 'tabular-nums',
})

const tagListStyle = css({
  display: 'flex',
  alignItems: 'center',
  gap: '1',
  flexWrap: 'wrap',
})

const tagChipStyle = css({
  display: 'inline-block',
  paddingX: '2',
  paddingY: '0.5',
  borderRadius: 'sm',
  fontSize: 'xs',
  border: '1px solid',
  borderColor: 'border',
  color: 'fg',
  opacity: '0.8',
})

const updatedStyle = css({
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
  whiteSpace: 'nowrap',
})

interface TaskSummaryCardProps {
  task: Task
  href?: string
}

export function TaskSummaryCard({ task, href }: TaskSummaryCardProps) {
  const { t } = useTranslation()
  const Component = href ? 'a' : 'div'
  const props = href ? { href } : {}

  return (
    <Component
      className={cardStyle}
      data-testid={`task-card-${task.id}`}
      {...props}
    >
      <div className={headerRowStyle}>
        <span className={titleStyle}>
          #{task.id} {task.title}
        </span>
        <span className={updatedStyle}>{formatDate(task.updated_at)}</span>
      </div>
      <div className={metaRowStyle}>
        <TaskStatusBadge status={task.status} />
        <span className={priorityStyle}>
          {t(`tasks.priority.${task.priority}`, { defaultValue: task.priority })}
        </span>
        {task.tags.length > 0 ? (
          <span className={tagListStyle}>
            {task.tags.map((tag) => (
              <span key={tag} className={tagChipStyle}>
                {tag}
              </span>
            ))}
          </span>
        ) : null}
      </div>
    </Component>
  )
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toISOString().slice(0, 10)
  } catch {
    return iso
  }
}
