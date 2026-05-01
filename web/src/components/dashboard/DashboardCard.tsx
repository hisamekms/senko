import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { css } from '../../../styled-system/css'

const cardStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
  padding: '4',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
  minHeight: '180px',
})

const headerStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
})

const titleStyle = css({
  fontSize: 'md',
  fontWeight: 'semibold',
})

const stateStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

const errorStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
  fontSize: 'sm',
  color: 'fg',
})

const errorRowStyle = css({
  color: 'accent',
})

const retryButtonStyle = css({
  alignSelf: 'flex-start',
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

const skeletonRowStyle = css({
  height: '12px',
  borderRadius: 'sm',
  backgroundColor: 'border',
  opacity: '0.6',
  animation: 'pulse 1.4s ease-in-out infinite',
})

const skeletonContainerStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '2',
})

const emptyStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
})

interface DashboardCardProps {
  title: string
  loading: boolean
  error: Error | null
  empty?: boolean
  emptyMessage?: string
  onRetry?: () => void
  children?: ReactNode
  headerRight?: ReactNode
}

export function DashboardCard({
  title,
  loading,
  error,
  empty,
  emptyMessage,
  onRetry,
  children,
  headerRight,
}: DashboardCardProps) {
  const { t } = useTranslation()

  return (
    <section className={cardStyle} aria-busy={loading}>
      <div className={headerStyle}>
        <h2 className={titleStyle}>{title}</h2>
        {headerRight ? <span className={stateStyle}>{headerRight}</span> : null}
      </div>
      {loading ? (
        <div className={skeletonContainerStyle} aria-label={t('dashboard.loading')}>
          <span className={skeletonRowStyle} style={{ width: '90%' }} />
          <span className={skeletonRowStyle} style={{ width: '70%' }} />
          <span className={skeletonRowStyle} style={{ width: '80%' }} />
        </div>
      ) : error ? (
        <div className={errorStyle} role="alert">
          <span className={errorRowStyle}>{t('dashboard.error')}</span>
          <span className={stateStyle}>{error.message}</span>
          {onRetry ? (
            <button type="button" className={retryButtonStyle} onClick={onRetry}>
              {t('dashboard.retry')}
            </button>
          ) : null}
        </div>
      ) : empty ? (
        <p className={emptyStyle}>{emptyMessage}</p>
      ) : (
        children
      )}
    </section>
  )
}
