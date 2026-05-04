import { useTranslation } from 'react-i18next'

import { DashboardCard } from '#/components/dashboard/DashboardCard'
import { fetchContracts } from '#/components/dashboard/fetchers'
import { useApi } from '#/hooks/useApi'
import { type components } from '#/api'
import { pickFillWidthBucket } from '#/utils/style/fillWidthBuckets'
import { css, cx } from '../../../styled-system/css'

type Contract = components['schemas']['ContractResponse']

const listStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const rowStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
  paddingX: '2',
  paddingY: '2',
  borderRadius: 'sm',
  color: 'fg',
  textDecoration: 'none',
  _hover: { backgroundColor: 'bg' },
})

const titleRowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
})

const titleStyle = css({
  fontSize: 'sm',
  fontWeight: 'medium',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

const progressLabelStyle = css({
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  fontVariantNumeric: 'tabular-nums',
  whiteSpace: 'nowrap',
})

const barStyle = css({
  position: 'relative',
  height: '6px',
  borderRadius: 'full',
  backgroundColor: 'border',
  overflow: 'hidden',
})

const fillStyle = css({
  position: 'absolute',
  top: '0',
  left: '0',
  height: '100%',
  backgroundColor: 'accent',
  borderRadius: 'full',
  transition: 'width 200ms ease',
})

const completedTagStyle = css({
  fontSize: 'xs',
  color: 'accent',
  fontWeight: 'semibold',
})

const moreLinkStyle = css({
  display: 'block',
  marginTop: '2',
  paddingX: '2',
  paddingY: '1',
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.7',
  textDecoration: 'none',
  textAlign: 'right',
  _hover: { opacity: '1', textDecoration: 'underline' },
})

interface ContractsCardProps {
  projectId: number
}

export function ContractsCard({ projectId }: ContractsCardProps) {
  const { t } = useTranslation()

  const { data, error, loading, reload } = useApi<Contract[]>(
    () => fetchContracts(projectId),
    [projectId],
  )

  const contracts = data ?? []

  return (
    <DashboardCard
      title={t('dashboard.contracts.title')}
      loading={loading}
      error={error}
      empty={!loading && !error && contracts.length === 0}
      emptyMessage={t('dashboard.contracts.empty')}
      onRetry={reload}
    >
      <ul className={listStyle}>
        {contracts.map((contract) => {
          const total = contract.definition_of_done.length
          const done = contract.definition_of_done.filter((d) => d.checked).length
          const percent = total === 0 ? 0 : Math.round((done / total) * 100)
          return (
            <li key={contract.id}>
              <a
                href={`/p/${projectId}/contracts/${contract.id}`}
                className={rowStyle}
                data-testid={`contract-row-${contract.id}`}
              >
                <div className={titleRowStyle}>
                  <span className={titleStyle}>
                    #{contract.id} {contract.title}
                  </span>
                  <span className={progressLabelStyle}>
                    {t('dashboard.contracts.progress', { done, total })}
                    {contract.is_completed ? (
                      <>
                        {' '}
                        <span className={completedTagStyle}>
                          ({t('dashboard.contracts.completed')})
                        </span>
                      </>
                    ) : null}
                  </span>
                </div>
                <div
                  className={barStyle}
                  role="progressbar"
                  aria-valuenow={percent}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <div
                    className={cx(fillStyle, pickFillWidthBucket(percent))}
                  />
                </div>
              </a>
            </li>
          )
        })}
      </ul>
      <a
        href={`/p/${projectId}/contracts`}
        className={moreLinkStyle}
        data-testid="contracts-more-link"
      >
        {t('dashboard.contracts.more')}
      </a>
    </DashboardCard>
  )
}
