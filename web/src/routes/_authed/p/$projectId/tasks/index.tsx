import { useCallback } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { TaskFilters, type TaskFilterValues } from '#/components/tasks/TaskFilters'
import { TaskSummaryCard } from '#/components/tasks'
import { useCursorList } from '#/hooks/useCursorList'
import { apiClient, type components } from '#/api'
import { css } from '../../../../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

interface TasksSearch {
  status?: string[]
  tag?: string
  ready?: boolean
  contract?: number
}

function asStringArray(v: unknown): string[] | undefined {
  if (Array.isArray(v)) {
    const out = v.filter((x): x is string => typeof x === 'string')
    return out.length > 0 ? out : undefined
  }
  if (typeof v === 'string') {
    return v.length > 0 ? [v] : undefined
  }
  return undefined
}

function asString(v: unknown): string | undefined {
  return typeof v === 'string' && v.length > 0 ? v : undefined
}

function asBoolean(v: unknown): boolean | undefined {
  if (typeof v === 'boolean') return v ? true : undefined
  if (typeof v === 'string') {
    if (v === 'true') return true
    if (v === 'false') return undefined
  }
  return undefined
}

function asNumber(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v
  if (typeof v === 'string' && v.length > 0) {
    const n = Number(v)
    if (Number.isFinite(n) && Number.isInteger(n) && n > 0) return n
  }
  return undefined
}

export const Route = createFileRoute('/_authed/p/$projectId/tasks/')({
  validateSearch: (raw: Record<string, unknown>): TasksSearch => ({
    status: asStringArray(raw.status),
    tag: asString(raw.tag),
    ready: asBoolean(raw.ready),
    contract: asNumber(raw.contract),
  }),
  component: TaskListPage,
})

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '4',
})

const headerStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
  flexWrap: 'wrap',
})

const titleStyle = css({
  fontSize: '2xl',
  fontWeight: 'bold',
  color: 'fg',
})

const countStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
})

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
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
  height: '60px',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  opacity: '0.6',
  animation: 'pulse 1.4s ease-in-out infinite',
})

const skeletonContainerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const loadMoreStyle = css({
  alignSelf: 'flex-start',
  paddingX: '4',
  paddingY: '2',
  fontSize: 'sm',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  color: 'fg',
  cursor: 'pointer',
  _hover: { borderColor: 'accent' },
  '&:disabled': { cursor: 'default', opacity: '0.5' },
})

function TaskListPage() {
  const { t } = useTranslation()
  const { projectId } = Route.useParams()
  const search = Route.useSearch()
  const navigate = useNavigate({ from: Route.fullPath })
  const numericId = Number(projectId)

  const fetchPage = useCallback(
    async (cursor: string | null) => {
      const query: Record<string, unknown> = {}
      if (search.status && search.status.length > 0) query.status = search.status
      if (search.tag) query.tag = [search.tag]
      if (search.ready) query.ready = true
      if (search.contract != null) query.contract = search.contract
      if (cursor) query.after = cursor

      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: numericId },
            query,
          },
        },
      )
      if (error || !data) throw new Error('Failed to load tasks')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    },
    [numericId, search.contract, search.ready, search.status, search.tag],
  )

  const { items, loading, error, hasMore, loadMore, reload } = useCursorList<Task>(
    fetchPage,
    [
      numericId,
      // serialize array deps so reference identity changes when contents differ
      JSON.stringify(search.status ?? []),
      search.tag ?? '',
      search.ready ?? false,
      search.contract ?? 0,
    ],
  )

  const onFiltersChange = (next: TaskFilterValues) => {
    navigate({
      search: () => ({
        status: next.status,
        tag: next.tag,
        ready: next.ready,
        contract: next.contract,
      }),
      replace: true,
    })
  }

  if (!Number.isFinite(numericId)) {
    return null
  }

  return (
    <div className={containerStyle}>
      <div className={headerStyle}>
        <h1 className={titleStyle}>{t('tasks.list.title')}</h1>
        {!loading && !error ? (
          <span className={countStyle}>
            {t('tasks.list.count', { count: items.length })}
          </span>
        ) : null}
      </div>
      <TaskFilters value={search} onChange={onFiltersChange} />

      {error ? (
        <div>
          <p className={errorStyle} role="alert">
            {error.message}
          </p>
          <button type="button" className={loadMoreStyle} onClick={reload}>
            {t('dashboard.retry')}
          </button>
        </div>
      ) : null}

      {!error && loading && items.length === 0 ? (
        <div className={skeletonContainerStyle} aria-label={t('dashboard.loading')}>
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
        </div>
      ) : null}

      {!error && !loading && items.length === 0 ? (
        <p className={stateStyle}>{t('tasks.list.empty')}</p>
      ) : null}

      {items.length > 0 ? (
        <ul className={listStyle}>
          {items.map((task) => (
            <li key={task.id}>
              <TaskSummaryCard
                task={task}
                href={`/p/${projectId}/tasks/${task.id}`}
              />
            </li>
          ))}
        </ul>
      ) : null}

      {hasMore && !error ? (
        <button
          type="button"
          className={loadMoreStyle}
          onClick={loadMore}
          disabled={loading}
          data-testid="task-list-load-more"
        >
          {loading ? t('dashboard.loading') : t('tasks.list.loadMore')}
        </button>
      ) : null}
    </div>
  )
}
