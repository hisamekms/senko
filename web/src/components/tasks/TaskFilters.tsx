import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { css } from '../../../styled-system/css'

export interface TaskFilterValues {
  status?: string[]
  tag?: string
  ready?: boolean
  contract?: number
  title?: string
  priority?: string[]
}

interface TaskFiltersProps {
  value: TaskFilterValues
  onChange: (next: TaskFilterValues) => void
}

const STATUSES = [
  'draft',
  'todo',
  'in_progress',
  'completed',
  'canceled',
] as const

const PRIORITIES = ['P0', 'P1', 'P2', 'P3'] as const

const containerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
  padding: '4',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
})

const sectionTitleStyle = css({
  fontSize: 'sm',
  fontWeight: 'semibold',
  color: 'fg',
})

const rowStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: '2',
})

const statusChipStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '1',
  paddingX: '2',
  paddingY: '1',
  borderRadius: 'full',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  color: 'fg',
  cursor: 'pointer',
  fontSize: 'sm',
  userSelect: 'none',
  '&[data-active="true"]': {
    borderColor: 'accent',
    color: 'accent',
  },
})

const inlineLabelStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '2',
  fontSize: 'sm',
  color: 'fg',
})

const inputStyle = css({
  paddingX: '2',
  paddingY: '1',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  color: 'fg',
  fontSize: 'sm',
  minWidth: '8rem',
})

const numberInputStyle = css({
  paddingX: '2',
  paddingY: '1',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  color: 'fg',
  fontSize: 'sm',
  width: '6rem',
})

const resetButtonStyle = css({
  paddingX: '3',
  paddingY: '1',
  fontSize: 'sm',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'bg',
  color: 'fg',
  cursor: 'pointer',
  _hover: { borderColor: 'accent' },
})

const fieldRowStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: '4',
})

export function TaskFilters({ value, onChange }: TaskFiltersProps) {
  const { t } = useTranslation()
  const [tagDraft, setTagDraft] = useState<string>(value.tag ?? '')
  const [contractDraft, setContractDraft] = useState<string>(
    value.contract != null ? String(value.contract) : '',
  )
  const [titleDraft, setTitleDraft] = useState<string>(value.title ?? '')

  useEffect(() => {
    setTagDraft(value.tag ?? '')
  }, [value.tag])
  useEffect(() => {
    setContractDraft(value.contract != null ? String(value.contract) : '')
  }, [value.contract])
  useEffect(() => {
    setTitleDraft(value.title ?? '')
  }, [value.title])

  const toggleStatus = (s: string) => {
    const current = value.status ?? []
    const next = current.includes(s)
      ? current.filter((x) => x !== s)
      : [...current, s]
    onChange({ ...value, status: next.length > 0 ? next : undefined })
  }

  const togglePriority = (p: string) => {
    const current = value.priority ?? []
    const next = current.includes(p)
      ? current.filter((x) => x !== p)
      : [...current, p]
    onChange({ ...value, priority: next.length > 0 ? next : undefined })
  }

  const submitTag = () => {
    const trimmed = tagDraft.trim()
    onChange({ ...value, tag: trimmed.length > 0 ? trimmed : undefined })
  }

  const submitTitle = () => {
    const trimmed = titleDraft.trim()
    onChange({ ...value, title: trimmed.length > 0 ? trimmed : undefined })
  }

  const submitContract = () => {
    const trimmed = contractDraft.trim()
    if (trimmed.length === 0) {
      onChange({ ...value, contract: undefined })
      return
    }
    const n = Number(trimmed)
    if (!Number.isFinite(n) || !Number.isInteger(n) || n <= 0) {
      onChange({ ...value, contract: undefined })
      return
    }
    onChange({ ...value, contract: n })
  }

  const reset = () => {
    onChange({})
  }

  const activeStatuses = new Set(value.status ?? [])
  const activePriorities = new Set(value.priority ?? [])

  return (
    <div className={containerStyle}>
      <span className={sectionTitleStyle}>{t('tasks.filter.status')}</span>
      <div className={rowStyle}>
        {STATUSES.map((s) => (
          <button
            key={s}
            type="button"
            className={statusChipStyle}
            data-active={activeStatuses.has(s) ? 'true' : 'false'}
            data-testid={`task-filter-status-${s}`}
            onClick={() => toggleStatus(s)}
          >
            {t(`dashboard.status.${s}`, { defaultValue: s })}
          </button>
        ))}
      </div>
      <span className={sectionTitleStyle}>{t('tasks.filter.priority')}</span>
      <div className={rowStyle}>
        {PRIORITIES.map((p) => (
          <button
            key={p}
            type="button"
            className={statusChipStyle}
            data-active={activePriorities.has(p) ? 'true' : 'false'}
            data-testid={`task-filter-priority-${p}`}
            onClick={() => togglePriority(p)}
          >
            {t(`tasks.priority.${p}`, { defaultValue: p })}
          </button>
        ))}
      </div>
      <div className={fieldRowStyle}>
        <label className={inlineLabelStyle}>
          {t('tasks.filter.title')}
          <input
            type="text"
            className={inputStyle}
            placeholder={t('tasks.filter.titlePlaceholder')}
            value={titleDraft}
            onChange={(e) => setTitleDraft(e.target.value)}
            onBlur={submitTitle}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                submitTitle()
              }
            }}
            data-testid="task-filter-title"
          />
        </label>
        <label className={inlineLabelStyle}>
          {t('tasks.filter.tag')}
          <input
            type="text"
            className={inputStyle}
            placeholder={t('tasks.filter.tagPlaceholder')}
            value={tagDraft}
            onChange={(e) => setTagDraft(e.target.value)}
            onBlur={submitTag}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                submitTag()
              }
            }}
            data-testid="task-filter-tag"
          />
        </label>
        <label className={inlineLabelStyle}>
          <input
            type="checkbox"
            checked={value.ready === true}
            onChange={(e) =>
              onChange({
                ...value,
                ready: e.target.checked ? true : undefined,
              })
            }
            data-testid="task-filter-ready"
          />
          {t('tasks.filter.ready')}
        </label>
        <label className={inlineLabelStyle}>
          {t('tasks.filter.contract')}
          <input
            type="number"
            min="1"
            className={numberInputStyle}
            value={contractDraft}
            onChange={(e) => setContractDraft(e.target.value)}
            onBlur={submitContract}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                submitContract()
              }
            }}
            data-testid="task-filter-contract"
          />
        </label>
        <button
          type="button"
          className={resetButtonStyle}
          onClick={reset}
          data-testid="task-filter-reset"
        >
          {t('tasks.filter.reset')}
        </button>
      </div>
    </div>
  )
}
