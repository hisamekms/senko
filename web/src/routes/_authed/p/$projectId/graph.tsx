import { useCallback, useEffect, useMemo, useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { GraphFilters, TaskGraph, type GraphFilterValues } from '#/components/graph'
import { TaskSummaryCard } from '#/components/tasks'
import { useApi } from '#/hooks/useApi'
import { useTheme } from '#/hooks/useTheme'
import { apiClient, collectAll, type components } from '#/api'
import { css, cx } from '../../../../../styled-system/css'

type Task = components['schemas']['TaskResponse']

interface GraphSearch {
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

export const Route = createFileRoute('/_authed/p/$projectId/graph')({
  validateSearch: (raw: Record<string, unknown>): GraphSearch => ({
    status: asStringArray(raw.status),
    tag: asString(raw.tag),
    ready: asBoolean(raw.ready),
    contract: asNumber(raw.contract),
  }),
  component: GraphPage,
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

const layoutStyle = css({
  display: 'flex',
  flexDirection: { base: 'column', lg: 'row' },
  gap: '4',
  alignItems: 'stretch',
})

const canvasWrapStyle = css({
  flex: '1',
  minHeight: '70vh',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  overflow: 'hidden',
  position: 'relative',
})

const sidePanelStyle = css({
  width: { base: 'auto', lg: '320px' },
  flexShrink: 0,
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
  padding: '4',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
})

const panelTitleStyle = css({
  fontSize: 'sm',
  fontWeight: 'semibold',
  color: 'fg',
})

const hintStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

const openLinkStyle = css({
  alignSelf: 'flex-start',
  paddingX: '3',
  paddingY: '1',
  fontSize: 'sm',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'accent',
  color: 'accent',
  textDecoration: 'none',
  _hover: { backgroundColor: 'accent', color: 'bg' },
})

const stateStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

const padding1RemStyle = css({ padding: '1rem' })

const errorStyle = css({
  fontSize: 'sm',
  color: 'accent',
})

const skeletonCanvasStyle = css({
  height: '70vh',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  opacity: '0.6',
  animation: 'pulse 1.4s ease-in-out infinite',
})

const retryButtonStyle = css({
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
})

function applyFilters(tasks: Task[], filters: GraphSearch): Task[] {
  const completedIds = new Set(
    tasks.filter((t) => t.status === 'completed').map((t) => t.id),
  )
  return tasks.filter((task) => {
    if (filters.status && filters.status.length > 0) {
      if (!filters.status.includes(task.status)) return false
    }
    if (filters.tag) {
      if (!task.tags.includes(filters.tag)) return false
    }
    if (filters.ready) {
      const allDone = task.dependencies.every((id) => completedIds.has(id))
      const eligible = task.status === 'todo'
      if (!eligible || !allDone) return false
    }
    if (filters.contract != null) {
      if (task.contract_id !== filters.contract) return false
    }
    return true
  })
}

function GraphPage() {
  const { t } = useTranslation()
  const { projectId } = Route.useParams()
  const search = Route.useSearch()
  const navigate = useNavigate({ from: Route.fullPath })
  const numericId = Number(projectId)
  const validId = Number.isFinite(numericId)
  const { theme } = useTheme()

  const [hasMounted, setHasMounted] = useState(false)
  useEffect(() => setHasMounted(true), [])

  const [selectedId, setSelectedId] = useState<number | null>(null)

  const fetchAll = useCallback(async (): Promise<Task[]> => {
    return collectAll<Task>(async (cursor) => {
      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: numericId },
            query: cursor ? { after: cursor } : {},
          },
        },
      )
      if (error || !data) throw new Error('Failed to load tasks')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    })
  }, [numericId])

  const { data, error, loading, reload } = useApi<Task[]>(fetchAll, [numericId])

  const visibleTasks = useMemo(() => {
    if (!data) return []
    return applyFilters(data, search)
  }, [data, search.status, search.tag, search.ready, search.contract])

  const selectedTask = useMemo(() => {
    if (selectedId == null || !data) return null
    return data.find((task) => task.id === selectedId) ?? null
  }, [data, selectedId])

  const onFiltersChange = (next: GraphFilterValues) => {
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

  if (!validId) {
    return null
  }

  return (
    <div className={containerStyle}>
      <div className={headerStyle}>
        <h1 className={titleStyle}>{t('graph.title')}</h1>
        {!loading && !error && data ? (
          <span className={countStyle}>
            {t('graph.count', {
              visible: visibleTasks.length,
              total: data.length,
            })}
          </span>
        ) : null}
      </div>

      <GraphFilters value={search} onChange={onFiltersChange} />

      {error ? (
        <div>
          <p className={errorStyle} role="alert">
            {error.message}
          </p>
          <button type="button" className={retryButtonStyle} onClick={reload}>
            {t('dashboard.retry')}
          </button>
        </div>
      ) : null}

      {!error && loading ? (
        <div
          className={skeletonCanvasStyle}
          aria-label={t('dashboard.loading')}
        />
      ) : null}

      {!error && !loading && data ? (
        <div className={layoutStyle}>
          <div className={canvasWrapStyle} data-testid="graph-canvas">
            {visibleTasks.length === 0 ? (
              <p
                className={cx(stateStyle, padding1RemStyle)}
                data-testid="graph-empty"
              >
                {t('graph.empty')}
              </p>
            ) : hasMounted ? (
              <TaskGraph
                tasks={visibleTasks}
                selectedId={selectedId}
                onSelect={setSelectedId}
                theme={theme}
              />
            ) : null}
          </div>
          <aside className={sidePanelStyle} data-testid="graph-side-panel">
            <span className={panelTitleStyle}>{t('graph.detailTitle')}</span>
            {selectedTask ? (
              <>
                <TaskSummaryCard task={selectedTask} />
                <a
                  className={openLinkStyle}
                  href={`/p/${projectId}/tasks/${selectedTask.id}`}
                  data-testid="graph-open-detail"
                >
                  {t('graph.openDetail')}
                </a>
              </>
            ) : (
              <p className={hintStyle}>{t('graph.selectHint')}</p>
            )}
          </aside>
        </div>
      ) : null}
    </div>
  )
}
