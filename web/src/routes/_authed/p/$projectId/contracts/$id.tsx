import { useCallback } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { ContractNotesTimeline, ContractProgressBar } from '#/components/contracts'
import { Markdown } from '#/components/markdown/Markdown'
import { TaskSummaryCard } from '#/components/tasks'
import { useApi } from '#/hooks/useApi'
import { useCursorList } from '#/hooks/useCursorList'
import { apiClient, type components } from '#/api'
import { css, cx } from '../../../../../../styled-system/css'

type Contract = components['schemas']['ContractResponse']
type Task = components['schemas']['TaskResponse']

export const Route = createFileRoute('/_authed/p/$projectId/contracts/$id')({
  component: ContractDetailPage,
})

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '5',
})

const headerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const idLabelStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
})

const titleStyle = css({
  fontSize: '2xl',
  fontWeight: 'bold',
  color: 'fg',
})

const tagsRowStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  gap: '2',
  alignItems: 'center',
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

const stateStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

const errorStyle = css({
  fontSize: 'sm',
  color: 'accent',
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

const taskListStyle = css({
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

function ContractDetailPage() {
  const { t } = useTranslation()
  const { projectId, id } = Route.useParams()
  const numericProjectId = Number(projectId)
  const numericId = Number(id)
  const validIds =
    Number.isFinite(numericProjectId) && Number.isFinite(numericId)

  const fetchContract = useCallback(async (): Promise<Contract> => {
    const { data, error } = await apiClient.GET(
      '/api/v1/projects/{project_id}/contracts/{id}',
      {
        params: {
          path: { project_id: numericProjectId, id: numericId },
        },
      },
    )
    if (error || !data) throw new Error('Failed to load contract')
    return data
  }, [numericProjectId, numericId])

  const fetchRelatedTasks = useCallback(
    async (cursor: string | null) => {
      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/tasks',
        {
          params: {
            path: { project_id: numericProjectId },
            query: {
              contract: numericId,
              ...(cursor ? { after: cursor } : {}),
            },
          },
        },
      )
      if (error || !data) throw new Error('Failed to load related tasks')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    },
    [numericProjectId, numericId],
  )

  const contractState = useApi<Contract>(fetchContract, [
    numericProjectId,
    numericId,
  ])
  const tasksState = useCursorList<Task>(fetchRelatedTasks, [
    numericProjectId,
    numericId,
  ])

  if (!validIds) {
    return null
  }

  if (contractState.loading) {
    return (
      <div className={skeletonContainerStyle} aria-label={t('dashboard.loading')}>
        <span className={cx(skeletonRowStyle, skeletonW60Style)} />
        <span className={cx(skeletonRowStyle, skeletonW90Style)} />
        <span className={cx(skeletonRowStyle, skeletonW80Style)} />
      </div>
    )
  }

  if (contractState.error) {
    return (
      <div className={sectionStyle}>
        <p className={errorStyle} role="alert">
          {contractState.error.message}
        </p>
        <button
          type="button"
          className={retryButtonStyle}
          onClick={contractState.reload}
        >
          {t('dashboard.retry')}
        </button>
      </div>
    )
  }

  const contract = contractState.data
  if (!contract) {
    return <p className={stateStyle}>{t('contracts.detail.notFound')}</p>
  }

  const total = contract.definition_of_done.length
  const done = contract.definition_of_done.filter((d) => d.checked).length
  const hasMetadata =
    contract.metadata != null && Object.keys(contract.metadata).length > 0

  return (
    <div className={containerStyle}>
      {/* Header */}
      <div className={headerStyle}>
        <span className={idLabelStyle}>#{contract.id}</span>
        <h1 className={titleStyle}>{contract.title}</h1>
        {contract.tags.length > 0 ? (
          <div className={tagsRowStyle}>
            {contract.tags.map((tag) => (
              <span key={tag} className={tagChipStyle}>
                {tag}
              </span>
            ))}
          </div>
        ) : null}
        <ContractProgressBar
          done={done}
          total={total}
          isCompleted={contract.is_completed}
        />
      </div>

      {/* Description */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('contracts.detail.description')}</h2>
        {contract.description ? (
          <Markdown source={contract.description} />
        ) : (
          <p className={stateStyle}>{t('contracts.detail.descriptionEmpty')}</p>
        )}
      </section>

      {/* DoD */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('contracts.detail.dod')}</h2>
        {contract.definition_of_done.length === 0 ? (
          <p className={stateStyle}>{t('contracts.detail.dodEmpty')}</p>
        ) : (
          <ul className={dodListStyle}>
            {contract.definition_of_done.map((dod, i) => (
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
        )}
      </section>

      {/* Metadata */}
      {hasMetadata ? (
        <section className={sectionStyle}>
          <h2 className={sectionTitleStyle}>{t('contracts.detail.metadata')}</h2>
          <div className={metaTableStyle}>
            {Object.entries(contract.metadata ?? {}).map(([k, v]) => (
              <FragmentRow key={k} label={k} value={String(v)} />
            ))}
          </div>
        </section>
      ) : null}

      {/* Related tasks */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>
          {t('contracts.detail.relatedTasks')}
        </h2>
        {tasksState.error && tasksState.items.length === 0 ? (
          <div>
            <p className={errorStyle} role="alert">
              {tasksState.error.message}
            </p>
            <button
              type="button"
              className={retryButtonStyle}
              onClick={tasksState.reload}
            >
              {t('dashboard.retry')}
            </button>
          </div>
        ) : tasksState.loading && tasksState.items.length === 0 ? (
          <span className={cx(skeletonRowStyle, skeletonW40Style)} />
        ) : tasksState.items.length === 0 ? (
          <p className={stateStyle}>
            {t('contracts.detail.relatedTasksEmpty')}
          </p>
        ) : (
          <>
            <ul className={taskListStyle}>
              {tasksState.items.map((task) => (
                <li key={task.id}>
                  <TaskSummaryCard
                    task={task}
                    href={`/p/${projectId}/tasks/${task.id}`}
                  />
                </li>
              ))}
            </ul>
            {tasksState.hasMore ? (
              <button
                type="button"
                className={loadMoreStyle}
                onClick={tasksState.loadMore}
                disabled={tasksState.loading}
                data-testid="contract-related-tasks-load-more"
              >
                {tasksState.loading
                  ? t('dashboard.loading')
                  : t('contracts.detail.loadMore')}
              </button>
            ) : null}
          </>
        )}
      </section>

      {/* Notes */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('contracts.detail.notes')}</h2>
        <ContractNotesTimeline
          projectId={numericProjectId}
          contractId={numericId}
        />
      </section>

      {/* Timestamps */}
      <section className={sectionStyle}>
        <h2 className={sectionTitleStyle}>{t('contracts.detail.timestamps')}</h2>
        <div className={metaTableStyle}>
          <FragmentRow
            label={t('contracts.detail.created')}
            value={contract.created_at}
          />
          <FragmentRow
            label={t('contracts.detail.updated')}
            value={contract.updated_at}
          />
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
