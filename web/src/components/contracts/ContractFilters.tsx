import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { css } from '../../../styled-system/css'

export interface ContractFilterValues {
  tag?: string
  title?: string
  // undefined = All, false = In progress, true = Completed
  completed?: boolean
}

interface ContractFiltersProps {
  value: ContractFilterValues
  onChange: (next: ContractFilterValues) => void
}

const COMPLETION_OPTIONS = [
  { key: 'all', value: undefined as boolean | undefined },
  { key: 'in_progress', value: false },
  { key: 'completed', value: true },
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

const chipStyle = css({
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

export function ContractFilters({ value, onChange }: ContractFiltersProps) {
  const { t } = useTranslation()
  const [tagDraft, setTagDraft] = useState<string>(value.tag ?? '')
  const [titleDraft, setTitleDraft] = useState<string>(value.title ?? '')

  useEffect(() => {
    setTagDraft(value.tag ?? '')
  }, [value.tag])
  useEffect(() => {
    setTitleDraft(value.title ?? '')
  }, [value.title])

  const setCompleted = (next: boolean | undefined) => {
    onChange({ ...value, completed: next })
  }

  const submitTag = () => {
    const trimmed = tagDraft.trim()
    onChange({ ...value, tag: trimmed.length > 0 ? trimmed : undefined })
  }

  const submitTitle = () => {
    const trimmed = titleDraft.trim()
    onChange({ ...value, title: trimmed.length > 0 ? trimmed : undefined })
  }

  const reset = () => {
    onChange({})
  }

  return (
    <div className={containerStyle}>
      <span className={sectionTitleStyle}>{t('contracts.filter.completion')}</span>
      <div className={rowStyle}>
        {COMPLETION_OPTIONS.map((opt) => {
          const active = value.completed === opt.value
          return (
            <button
              key={opt.key}
              type="button"
              className={chipStyle}
              data-active={active ? 'true' : 'false'}
              data-testid={`contract-filter-completion-${opt.key}`}
              onClick={() => setCompleted(opt.value)}
            >
              {t(`contracts.filter.completion_${opt.key}`)}
            </button>
          )
        })}
      </div>
      <div className={fieldRowStyle}>
        <label className={inlineLabelStyle}>
          {t('contracts.filter.title')}
          <input
            type="text"
            className={inputStyle}
            placeholder={t('contracts.filter.titlePlaceholder')}
            value={titleDraft}
            onChange={(e) => setTitleDraft(e.target.value)}
            onBlur={submitTitle}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                submitTitle()
              }
            }}
            data-testid="contract-filter-title"
          />
        </label>
        <label className={inlineLabelStyle}>
          {t('contracts.filter.tag')}
          <input
            type="text"
            className={inputStyle}
            placeholder={t('contracts.filter.tagPlaceholder')}
            value={tagDraft}
            onChange={(e) => setTagDraft(e.target.value)}
            onBlur={submitTag}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                submitTag()
              }
            }}
            data-testid="contract-filter-tag"
          />
        </label>
        <button
          type="button"
          className={resetButtonStyle}
          onClick={reset}
          data-testid="contract-filter-reset"
        >
          {t('contracts.filter.reset')}
        </button>
      </div>
    </div>
  )
}
