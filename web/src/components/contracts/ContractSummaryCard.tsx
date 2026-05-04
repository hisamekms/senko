import { useTranslation } from 'react-i18next'

import { ContractProgressBar } from '#/components/contracts/ContractProgressBar'
import type { components } from '#/api'
import { css } from '../../../styled-system/css'

type Contract = components['schemas']['ContractResponse']

const cardStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
  padding: '4',
  borderRadius: 'sm',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
  textDecoration: 'none',
  _hover: { borderColor: 'accent' },
})

const headerRowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '3',
  flexWrap: 'wrap',
})

const titleStyle = css({
  fontSize: 'md',
  fontWeight: 'semibold',
  flex: '1',
  minWidth: '0',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

const taskCountStyle = css({
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
  whiteSpace: 'nowrap',
})

const tagListStyle = css({
  display: 'flex',
  flexWrap: 'wrap',
  gap: '1',
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
  opacity: '0.8',
})

interface ContractSummaryCardProps {
  contract: Contract
  href: string
  taskCount?: number
}

export function ContractSummaryCard({
  contract,
  href,
  taskCount,
}: ContractSummaryCardProps) {
  const { t } = useTranslation()
  const total = contract.definition_of_done.length
  const done = contract.definition_of_done.filter((d) => d.checked).length

  return (
    <a
      className={cardStyle}
      href={href}
      data-testid={`contract-card-${contract.id}`}
    >
      <div className={headerRowStyle}>
        <span className={titleStyle}>
          #{contract.id} {contract.title}
        </span>
        {taskCount != null ? (
          <span className={taskCountStyle}>
            {taskCount > 0
              ? t('contracts.list.tasks', { count: taskCount })
              : t('contracts.list.noTasks')}
          </span>
        ) : null}
      </div>
      {contract.tags.length > 0 ? (
        <div className={tagListStyle}>
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
    </a>
  )
}
