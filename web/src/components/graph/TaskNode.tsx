import { Handle, Position, type NodeProps } from '@xyflow/react'
import { useTranslation } from 'react-i18next'

import { TaskStatusBadge } from '#/components/tasks'
import type { components } from '#/api'
import { cva } from '../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

const node = cva({
  base: {
    width: '220px',
    minHeight: '90px',
    boxSizing: 'border-box',
    padding: '2',
    borderRadius: 'sm',
    border: '2px solid',
    backgroundColor: 'surface',
    color: 'fg',
    display: 'flex',
    flexDirection: 'column',
    gap: '1',
    fontSize: 'xs',
    cursor: 'pointer',
    transition: 'border-color 120ms ease',
  },
  variants: {
    status: {
      draft: { borderColor: 'border', opacity: '0.7' },
      todo: { borderColor: 'border' },
      in_progress: { borderColor: 'accent' },
      completed: { borderColor: 'accent', opacity: '0.85' },
      canceled: {
        borderColor: 'border',
        opacity: '0.5',
        textDecoration: 'line-through',
      },
      unknown: { borderColor: 'border' },
    },
    selected: {
      true: { boxShadow: '0 0 0 3px var(--colors-accent)' },
      false: {},
    },
  },
  defaultVariants: { status: 'unknown', selected: false },
})

const titleStyle = cva({
  base: {
    fontWeight: 'medium',
    fontSize: 'sm',
    color: 'fg',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
})

const metaStyle = cva({
  base: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '2',
    fontSize: 'xs',
    color: 'fg',
    opacity: '0.8',
  },
})

const idStyle = cva({
  base: {
    fontVariantNumeric: 'tabular-nums',
    color: 'fg',
    opacity: '0.7',
    fontSize: 'xs',
  },
})

const KNOWN = ['draft', 'todo', 'in_progress', 'completed', 'canceled'] as const
type Known = (typeof KNOWN)[number]

function normalize(s: string): Known | 'unknown' {
  return (KNOWN as readonly string[]).includes(s) ? (s as Known) : 'unknown'
}

export interface TaskNodeData extends Record<string, unknown> {
  task: Task
}

export function TaskNode({ data, selected }: NodeProps) {
  const { t } = useTranslation()
  const { task } = data as TaskNodeData
  const variant = normalize(task.status)

  return (
    <div
      className={node({ status: variant, selected: selected ? true : false })}
      data-testid={`graph-node-${task.id}`}
    >
      <Handle type="target" position={Position.Top} />
      <span className={idStyle()}>#{task.id}</span>
      <span className={titleStyle()}>{task.title}</span>
      <div className={metaStyle()}>
        <TaskStatusBadge status={task.status} />
        <span>
          {t(`tasks.priority.${task.priority}`, { defaultValue: task.priority })}
        </span>
      </div>
      <Handle type="source" position={Position.Bottom} />
    </div>
  )
}
