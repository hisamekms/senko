import { useTranslation } from 'react-i18next'

import { cva } from '../../../styled-system/css'

const badge = cva({
  base: {
    display: 'inline-block',
    paddingX: '2',
    paddingY: '0.5',
    borderRadius: 'full',
    fontSize: 'xs',
    fontWeight: 'semibold',
    border: '1px solid',
    whiteSpace: 'nowrap',
  },
  variants: {
    status: {
      draft: {
        backgroundColor: 'surface',
        borderColor: 'border',
        color: 'fg',
        opacity: '0.7',
      },
      todo: {
        backgroundColor: 'surface',
        borderColor: 'border',
        color: 'fg',
      },
      in_progress: {
        backgroundColor: 'accent',
        borderColor: 'accent',
        color: 'bg',
      },
      completed: {
        backgroundColor: 'surface',
        borderColor: 'accent',
        color: 'accent',
      },
      canceled: {
        backgroundColor: 'surface',
        borderColor: 'border',
        color: 'fg',
        opacity: '0.5',
        textDecoration: 'line-through',
      },
      unknown: {
        backgroundColor: 'surface',
        borderColor: 'border',
        color: 'fg',
      },
    },
  },
  defaultVariants: { status: 'unknown' },
})

const KNOWN = ['draft', 'todo', 'in_progress', 'completed', 'canceled'] as const
type Known = (typeof KNOWN)[number]

function normalize(s: string): Known | 'unknown' {
  return (KNOWN as readonly string[]).includes(s) ? (s as Known) : 'unknown'
}

interface TaskStatusBadgeProps {
  status: string
}

export function TaskStatusBadge({ status }: TaskStatusBadgeProps) {
  const { t } = useTranslation()
  const variant = normalize(status)
  const label =
    variant === 'unknown'
      ? status
      : t(`dashboard.status.${variant}`, { defaultValue: variant })
  return (
    <span
      className={badge({ status: variant })}
      data-testid={`task-status-${status}`}
    >
      {label}
    </span>
  )
}
