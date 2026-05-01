import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { css } from '../../../styled-system/css'

export interface GraphFilterValues {
  status?: string[]
  tag?: string
  ready?: boolean
  contract?: number
}

interface GraphFiltersProps {
  value: GraphFilterValues
  onChange: (next: GraphFilterValues) => void
}

const STATUSES = [
  'draft',
  'todo',
  'in_progress',
  'completed',
  'canceled',
] as const

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

export function GraphFilters({ value, onChange }: GraphFiltersProps) {
  const { t } = useTranslation()
  const [tagDraft, setTagDraft] = useState<string>(value.tag ?? '')
  const [contractDraft, setContractDraft] = useState<string>(
    value.contract != null ? String(value.contract) : '',
  )

  useEffect(() => {
    setTagDraft(value.tag ?? '')
  }, [value.tag])
  useEffect(() => {
    setContractDraft(value.contract != null ? String(value.contract) : '')
  }, [value.contract])

  const toggleStatus = (s: string) => {
    const current = value.status ?? []
    const next = current.includes(s)
      ? current.filter((x) => x !== s)
      : [...current, s]
    onChange({ ...value, status: next.length > 0 ? next : undefined })
  }

  const submitTag = () => {
    const trimmed = tagDraft.trim()
    onChange({ ...value, tag: trimmed.length > 0 ? trimmed : undefined })
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
            data-testid={`graph-filter-status-${s}`}
            onClick={() => toggleStatus(s)}
          >
            {t(`dashboard.status.${s}`, { defaultValue: s })}
          </button>
        ))}
      </div>
      <div className={fieldRowStyle}>
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
            data-testid="graph-filter-tag"
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
            data-testid="graph-filter-ready"
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
            data-testid="graph-filter-contract"
          />
        </label>
        <button
          type="button"
          className={resetButtonStyle}
          onClick={reset}
          data-testid="graph-filter-reset"
        >
          {t('tasks.filter.reset')}
        </button>
      </div>
    </div>
  )
}
