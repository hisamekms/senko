import { useCallback } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { ContractSummaryCard } from '#/components/contracts'
import { useApi } from '#/hooks/useApi'
import { apiClient, collectAll, type components } from '#/api'
import { css } from '../../../../../../styled-system/css'

type Contract = components['schemas']['ContractResponse']
type Task = components['schemas']['TaskResponse']

export const Route = createFileRoute('/_authed/p/$projectId/contracts/')({
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

interface ContractsListData {
  contracts: Contract[]
  taskCounts: Map<number, number>
}

function ContractListPage() {
  const { t } = useTranslation()
  const { projectId } = Route.useParams()
  const numericId = Number(projectId)
  const validId = Number.isFinite(numericId)

  const fetchAll = useCallback(async (): Promise<ContractsListData> => {
    const [contracts, tasks] = await Promise.all([
      collectAll<Contract>(async (cursor) => {
        const { data, error } = await apiClient.GET(
          '/api/v1/projects/{project_id}/contracts',
          {
            params: {
              path: { project_id: numericId },
              query: cursor ? { after: cursor } : {},
            },
          },
        )
        if (error || !data) throw new Error('Failed to load contracts')
        return { items: data.items, next_cursor: data.next_cursor ?? null }
      }),
      collectAll<Task>(async (cursor) => {
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
      }),
    ])

    const taskCounts = new Map<number, number>()
    for (const task of tasks) {
      if (task.contract_id != null) {
        taskCounts.set(
          task.contract_id,
          (taskCounts.get(task.contract_id) ?? 0) + 1,
        )
      }
    }
    return { contracts, taskCounts }
  }, [numericId])

  const { data, error, loading, reload } = useApi<ContractsListData>(fetchAll, [
    numericId,
  ])

  if (!validId) {
    return null
  }

  return (
    <div className={containerStyle}>
      <div className={headerStyle}>
        <h1 className={titleStyle}>{t('contracts.list.title')}</h1>
        {!loading && !error && data ? (
          <span className={countStyle}>
            {t('contracts.list.count', { count: data.contracts.length })}
          </span>
        ) : null}
      </div>

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
          className={skeletonContainerStyle}
          aria-label={t('dashboard.loading')}
        >
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
          <span className={skeletonRowStyle} />
        </div>
      ) : null}

      {!error && !loading && data && data.contracts.length === 0 ? (
        <p className={stateStyle}>{t('contracts.list.empty')}</p>
      ) : null}

      {!error && !loading && data && data.contracts.length > 0 ? (
        <ul className={listStyle}>
          {data.contracts.map((contract) => (
            <li key={contract.id}>
              <ContractSummaryCard
                contract={contract}
                href={`/p/${projectId}/contracts/${contract.id}`}
                taskCount={data.taskCounts.get(contract.id) ?? 0}
              />
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
