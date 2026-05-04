import { useCallback } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import {
  ContractFilters,
  ContractSummaryCard,
  type ContractFilterValues,
} from '#/components/contracts'
import { useCursorList } from '#/hooks/useCursorList'
import { apiClient, type components } from '#/api'
import { asBoolean, asOrder, asString } from '#/api/searchParams'
import { css } from '../../../../../../styled-system/css'

type Contract = components['schemas']['ContractResponse']

interface ContractsSearch {
  tag?: string
  title?: string
  completed?: boolean
  order_by?: string
  order?: 'asc' | 'desc'
}

export const Route = createFileRoute('/_authed/p/$projectId/contracts/')({
  validateSearch: (raw: Record<string, unknown>): ContractsSearch => ({
    tag: asString(raw.tag),
    title: asString(raw.title),
    completed: asBoolean(raw.completed),
    order_by: asString(raw.order_by),
    order: asOrder(raw.order),
  }),
  component: ContractListPage,
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
  gap: '3',
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
  height: '110px',
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
  gap: '3',
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

function ContractListPage() {
  const { t } = useTranslation()
  const { projectId } = Route.useParams()
  const search = Route.useSearch()
  const navigate = useNavigate({ from: Route.fullPath })
  const numericId = Number(projectId)

  const fetchPage = useCallback(
    async (cursor: string | null) => {
      const query: Record<string, unknown> = {
        // Default sort matches the dashboard fetcher; URL params override.
        order_by: search.order_by ?? 'updated_at',
        order: search.order ?? 'desc',
      }
      if (search.tag) query.tag = [search.tag]
      if (search.title) query.title = search.title
      if (search.completed != null) query.completed = search.completed
      if (cursor) query.after = cursor

      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/contracts',
        {
          params: {
            path: { project_id: numericId },
            query,
          },
        },
      )
      if (error || !data) throw new Error('Failed to load contracts')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    },
    [
      numericId,
      search.tag,
      search.title,
      search.completed,
      search.order_by,
      search.order,
    ],
  )

  const { items, loading, error, hasMore, loadMore, reload } =
    useCursorList<Contract>(fetchPage, [
      numericId,
      search.tag ?? '',
      search.title ?? '',
      search.completed ?? null,
      search.order_by ?? '',
      search.order ?? '',
    ])

  const onFiltersChange = (next: ContractFilterValues) => {
    navigate({
      // Preserve order_by/order (URL-only).
      search: () => ({
        tag: next.tag,
        title: next.title,
        completed: next.completed,
        order_by: search.order_by,
        order: search.order,
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
        <h1 className={titleStyle}>{t('contracts.list.title')}</h1>
        {!loading && !error ? (
          <span className={countStyle}>
            {t('contracts.list.count', { count: items.length })}
          </span>
        ) : null}
      </div>
      <ContractFilters value={search} onChange={onFiltersChange} />

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
        <div
          className={skeletonContainerStyle}
          aria-label={t('dashboard.loading')}
        >
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
        </div>
      ) : null}

      {!error && !loading && items.length === 0 ? (
        <p className={stateStyle}>{t('contracts.list.empty')}</p>
      ) : null}

      {items.length > 0 ? (
        <ul className={listStyle}>
          {items.map((contract) => (
            <li key={contract.id}>
              <ContractSummaryCard
                contract={contract}
                href={`/p/${projectId}/contracts/${contract.id}`}
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
          data-testid="contract-list-load-more"
        >
          {loading ? t('dashboard.loading') : t('contracts.list.loadMore')}
        </button>
      ) : null}
    </div>
  )
}
