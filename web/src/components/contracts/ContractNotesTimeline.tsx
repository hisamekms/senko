import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { Markdown } from '#/components/markdown/Markdown'
import { useCursorList } from '#/hooks/useCursorList'
import { apiClient, type components } from '#/api'
import { css } from '../../../styled-system/css'

type ContractNote = components['schemas']['ContractNoteResponse']

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
})

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
})

const noteStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
  padding: '3',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
})

const noteHeaderStyle = css({
  display: 'flex',
  alignItems: 'center',
  gap: '2',
  flexWrap: 'wrap',
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
})

const sourceChipStyle = css({
  display: 'inline-block',
  paddingX: '2',
  paddingY: '0.5',
  borderRadius: 'sm',
  fontSize: 'xs',
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
  height: '60px',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  opacity: '0.6',
  animation: 'pulse 1.4s ease-in-out infinite',
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

interface ContractNotesTimelineProps {
  projectId: number
  contractId: number
}

export function ContractNotesTimeline({
  projectId,
  contractId,
}: ContractNotesTimelineProps) {
  const { t } = useTranslation()

  const fetchPage = useCallback(
    async (cursor: string | null) => {
      const { data, error } = await apiClient.GET(
        '/api/v1/projects/{project_id}/contracts/{id}/notes',
        {
          params: {
            path: { project_id: projectId, id: contractId },
            query: cursor ? { after: cursor } : {},
          },
        },
      )
      if (error || !data) throw new Error('Failed to load notes')
      return { items: data.items, next_cursor: data.next_cursor ?? null }
    },
    [projectId, contractId],
  )

  const { items, loading, error, hasMore, loadMore, reload } =
    useCursorList<ContractNote>(fetchPage, [projectId, contractId])

  if (error && items.length === 0) {
    return (
      <div className={containerStyle}>
        <p className={errorStyle} role="alert">
          {error.message}
        </p>
        <button type="button" className={loadMoreStyle} onClick={reload}>
          {t('dashboard.retry')}
        </button>
      </div>
    )
  }

  if (loading && items.length === 0) {
    return (
      <div className={containerStyle} aria-label={t('dashboard.loading')}>
        <span className={skeletonRowStyle} />
        <span className={skeletonRowStyle} />
      </div>
    )
  }

  if (!loading && items.length === 0) {
    return <p className={stateStyle}>{t('contracts.detail.notesEmpty')}</p>
  }

  return (
    <div className={containerStyle}>
      <ol className={listStyle}>
        {items.map((note, i) => (
          <li
            key={`${note.created_at}-${i}`}
            className={noteStyle}
            data-testid={`contract-note-${i}`}
          >
            <div className={noteHeaderStyle}>
              <time dateTime={note.created_at}>
                {formatTimestamp(note.created_at)}
              </time>
              {note.source_task_id != null ? (
                <a
                  href={`/p/${projectId}/tasks/${note.source_task_id}`}
                  className={sourceChipStyle}
                  data-testid={`note-source-${note.source_task_id}`}
                >
                  {t('contracts.detail.noteSource', { id: note.source_task_id })}
                </a>
              ) : null}
            </div>
            <Markdown source={note.content} />
          </li>
        ))}
      </ol>
      {error ? (
        <p className={errorStyle} role="alert">
          {error.message}
        </p>
      ) : null}
      {hasMore ? (
        <button
          type="button"
          className={loadMoreStyle}
          onClick={loadMore}
          disabled={loading}
          data-testid="contract-notes-load-more"
        >
          {loading ? t('dashboard.loading') : t('contracts.detail.loadMore')}
        </button>
      ) : null}
    </div>
  )
}

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso)
    const date = d.toISOString().slice(0, 10)
    const time = d.toISOString().slice(11, 16)
    return `${date} ${time}`
  } catch {
    return iso
  }
}
