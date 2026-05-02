import { useCallback } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { Markdown } from '#/components/markdown/Markdown'
import { TaskStatusBadge } from '#/components/tasks'
import { useApi } from '#/hooks/useApi'
import { apiClient, collectAll, type components } from '#/api'
import { css, cx } from '../../../../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

export const Route = createFileRoute('/_authed/p/$projectId/tasks/$id')({
  component: TaskDetailPage,
})

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '5',
})

const titleRowStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const titleStyle = css({
  fontSize: '2xl',
  fontWeight: 'bold',
  color: 'fg',
})

const idLabelStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
})

const headerMetaRowStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  gap: '2',
  alignItems: 'center',
})

const priorityChipStyle = css({
  display: 'inline-block',
  paddingX: '2',
  paddingY: '0.5',
  borderRadius: 'full',
  fontSize: 'xs',
  fontWeight: 'semibold',
  border: '1px solid',
  borderColor: 'accent',
  color: 'accent',
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
  opacity: '0.85',
})

const sectionStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const sectionTitleStyle = css({
  fontSize: 'md',
  fontWeight: 'semibold',
  color: 'fg',
})

const metaTableStyle = css({
  display: 'grid',
  gridTemplateColumns: { base: '1fr', md: 'auto 1fr' },
  rowGap: '1',
  columnGap: '4',
  fontSize: 'sm',
})

const metaKeyStyle = css({
  color: 'fg',
  opacity: '0.7',
  fontWeight: 'medium',
  fontSize: 'sm',
  whiteSpace: 'nowrap',
})

const metaValueStyle = css({
  color: 'fg',
  fontSize: 'sm',
  wordBreak: 'break-word',
})

const linkStyle = css({
  color: 'accent',
  textDecoration: 'underline',
  _hover: { textDecoration: 'none' },
})

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
  paddingLeft: '5',
  fontSize: 'sm',
  color: 'fg',
  listStyleType: 'disc',
})

const dodListStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
})

const dodRowStyle = css({
  display: 'flex',
  alignItems: 'flex-start',
  gap: '2',
  fontSize: 'sm',
  color: 'fg',
})

const dodCheckStyle = css({
  display: 'inline-block',
  width: '14px',
  height: '14px',
  flexShrink: '0',
  marginTop: '0.5',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  '&[data-checked="true"]': {
    backgroundColor: 'accent',
    borderColor: 'accent',
  },
})

const chipListStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  gap: '2',
})

const depChipStyle = css({
  display: 'inline-block',
  paddingX: '2',
  paddingY: '0.5',
  borderRadius: 'sm',
  fontSize: 'sm',
  border: '1px solid',
  borderColor: 'border',
  color: 'accent',
  textDecoration: 'none',
  _hover: { borderColor: 'accent' },
})

const stateStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

const errorStyle = css({
  fontSize: 'sm',
  color: 'accent',
})

const skeletonRowStyle = css({
  height: '16px',
  borderRadius: 'sm',
  backgroundColor: 'border',
  opacity: '0.6',
  animation: 'pulse 1.4s ease-in-out infinite',
})

const skeletonW40Style = css({ width: '40%' })
const skeletonW60Style = css({ width: '60%' })
const skeletonW80Style = css({ width: '80%' })
const skeletonW90Style = css({ width: '90%' })

const skeletonContainerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
  padding: '4',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
})

const codeBlockStyle = css({
  fontFamily: 'mono',
  fontSize: 'sm',
  paddingX: '2',
  paddingY: '1',
  borderRadius: 'sm',
  backgroundColor: 'surface',
  border: '1px solid',
  borderColor: 'border',
  display: 'inline-block',
})

function TaskDetailPage() {
  const { t } = useTranslation()
  const { projectId, id } = Route.useParams()
  const numericProjectId = Number(projectId)
  const numericId = Number(id)
  const validIds = Number.isFinite(numericProjectId) && Number.isFinite(numericId)

  const fetchTask = useCallback(async (): Promise<Task> => {
    const { data, error } = await apiClient.GET(
      '/api/v1/projects/{project_id}/tasks/{id}',
      {
        params: {
          path: { project_id: numericProjectId, id: numericId },
        },
      },
    )
    if (error || !data) throw new Error('Failed to load task')
    return data
  }, [numericProjectId, numericId])

  const fetchDependents = useCallback(async (): Promise<Task[]> => {
    return collectAll<Task>(async (cursor) => {
      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: numericProjectId },
            query: {
              depends_on: numericId,
              ...(cursor ? { after: cursor } : {}),
            },
          },
        },
      )
      if (error || !data) throw new Error('Failed to load dependents')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    })
  }, [numericProjectId, numericId])

  const taskState = useApi<Task>(fetchTask, [numericProjectId, numericId])
  const dependentsState = useApi<Task[]>(fetchDependents, [
    numericProjectId,
    numericId,
  ])

  if (!validIds) {
    return null
  }

  if (taskState.loading) {
    return (
      <div className={skeletonContainerStyle} aria-label={t('dashboard.loading')}>
        <span className={cx(skeletonRowStyle, skeletonW60Style)} />
        <span className={cx(skeletonRowStyle, skeletonW90Style)} />
        <span className={cx(skeletonRowStyle, skeletonW80Style)} />
      </div>
    )
  }

  if (taskState.error) {
    return (
      <div className={sectionStyle}>
        <p className={errorStyle} role="alert">
          {taskState.error.message}
        </p>
        <button
          type="button"
          className={depChipStyle}
          onClick={taskState.reload}
        >
          {t('dashboard.retry')}
        </button>
      </div>
    )
  }

  const task = taskState.data
  if (!task) {
    return <p className={stateStyle}>{t('tasks.detail.notFound')}</p>
  }

  return (
    <div className={containerStyle}>
      {/* Header */}
      <div className={titleRowStyle}>
        <span className={idLabelStyle}>#{task.id}</span>
        <h1 className={titleStyle}>{task.title}</h1>
        <div className={headerMetaRowStyle}>
          <TaskStatusBadge status={task.status} />
          <span className={priorityChipStyle}>
            {t(`tasks.priority.${task.priority}`, {
              defaultValue: task.priority,
            })}
          </span>
          {task.tags.map((tag) => (
            <span key={tag} className={tagChipStyle}>
              {tag}
            </span>
          ))}
        </div>
      </div>

      {/* Summary metadata */}
      <section className={sectionStyle}>
        <div className={metaTableStyle}>
          {task.assignee_user_id != null ? (
            <>
              <span className={metaKeyStyle}>{t('tasks.detail.assignee')}</span>
              <span className={metaValueStyle}>#{task.assignee_user_id}</span>
            </>
          ) : null}
          {task.contract_id != null ? (
            <>
              <span className={metaKeyStyle}>{t('tasks.detail.contract')}</span>
              <span className={metaValueStyle}>
                <a
                  href={`/p/${projectId}/contracts/${task.contract_id}`}
                  className={linkStyle}
                  data-testid={`contract-link-${task.contract_id}`}
                >
                  #{task.contract_id}
                </a>
              </span>
            </>
          ) : null}
          {task.branch ? (
            <>
              <span className={metaKeyStyle}>{t('tasks.detail.branch')}</span>
              <span className={metaValueStyle}>
                <code className={codeBlockStyle}>{task.branch}</code>
              </span>
            </>
          ) : null}
          {task.pr_url ? (
            <>
              <span className={metaKeyStyle}>{t('tasks.detail.prUrl')}</span>
              <span className={metaValueStyle}>
                <a
                  href={task.pr_url}
                  className={linkStyle}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {task.pr_url}
                </a>
              </span>
            </>
          ) : null}
        </div>
      </section>

      {/* Background */}
      {task.background ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.background')}</h2>
          <Markdown source={task.background} />
        </section>
      ) : null}

      {/* Description */}
      {task.description ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.description')}</h2>
          <Markdown source={task.description} />
        </section>
      ) : null}

      {/* Plan */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('tasks.detail.plan')}</h2>
        {task.plan ? (
          <Markdown source={task.plan} />
        ) : (
          <p className={stateStyle}>{t('tasks.detail.planEmpty')}</p>
        )}
      </section>

      {/* In scope */}
      {task.in_scope.length > 0 ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.inScope')}</h2>
          <ul className={listStyle}>
            {task.in_scope.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </section>
      ) : null}

      {/* Out of scope */}
      {task.out_of_scope.length > 0 ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.outOfScope')}</h2>
          <ul className={listStyle}>
            {task.out_of_scope.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </section>
      ) : null}

      {/* DoD */}
      {task.definition_of_done.length > 0 ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.dod')}</h2>
          <ul className={dodListStyle}>
            {task.definition_of_done.map((dod, i) => (
              <li key={i} className={dodRowStyle}>
                <span
                  className={dodCheckStyle}
                  data-checked={dod.checked ? 'true' : 'false'}
                  aria-label={dod.checked ? 'checked' : 'unchecked'}
                />
                <span>{dod.content}</span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {/* Dependencies (this depends on) */}
      {task.dependencies.length > 0 ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.dependencies')}</h2>
          <div className={chipListStyle}>
            {task.dependencies.map((depId) => (
              <a
                key={depId}
                href={`/p/${projectId}/tasks/${depId}`}
                className={depChipStyle}
                data-testid={`dep-link-${depId}`}
              >
                #{depId}
              </a>
            ))}
          </div>
        </section>
      ) : null}

      {/* Dependents (blocked by this) */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('tasks.detail.dependents')}</h2>
        {dependentsState.loading ? (
          <span className={cx(skeletonRowStyle, skeletonW40Style)} />
        ) : dependentsState.error ? (
          <p className={errorStyle}>{dependentsState.error.message}</p>
        ) : (dependentsState.data ?? []).length === 0 ? (
          <p className={stateStyle}>—</p>
        ) : (
          <div className={chipListStyle}>
            {(dependentsState.data ?? []).map((dt) => (
              <a
                key={dt.id}
                href={`/p/${projectId}/tasks/${dt.id}`}
                className={depChipStyle}
                data-testid={`dependent-link-${dt.id}`}
              >
                #{dt.id} {dt.title}
              </a>
            ))}
          </div>
        )}
      </section>

      {/* Metadata */}
      {task.metadata && Object.keys(task.metadata).length > 0 ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('tasks.detail.metadata')}</h2>
          <div className={metaTableStyle}>
            {Object.entries(task.metadata).map(([k, v]) => (
              <FragmentRow key={k} label={k} value={String(v)} />
            ))}
          </div>
        </section>
      ) : null}

      {/* Timestamps */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('tasks.detail.timestamps')}</h2>
        <div className={metaTableStyle}>
          <FragmentRow label={t('tasks.detail.created')} value={task.created_at} />
          <FragmentRow label={t('tasks.detail.updated')} value={task.updated_at} />
          {task.started_at ? (
            <FragmentRow
              label={t('tasks.detail.started')}
              value={task.started_at}
            />
          ) : null}
          {task.completed_at ? (
            <FragmentRow
              label={t('tasks.detail.completed')}
              value={task.completed_at}
            />
          ) : null}
          {task.canceled_at ? (
            <FragmentRow
              label={t('tasks.detail.canceled')}
              value={task.canceled_at}
            />
          ) : null}
        </div>
      </section>
    </div>
  )
}

function FragmentRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <span className={metaKeyStyle}>{label}</span>
      <span className={metaValueStyle}>{value}</span>
    </>
  )
}
