import { useTranslation } from 'react-i18next'

import { pickFillWidthBucket } from '#/utils/style/fillWidthBuckets'
import { css, cx } from '../../../styled-system/css'

const wrapperStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
  minWidth: '0',
})

const labelRowStyle = css({
  display: 'flex',
  alignItems: 'baseline',
  justifyContent: 'space-between',
  gap: '2',
  fontSize: 'xs',
  color: 'fg',
  opacity: '0.8',
  fontVariantNumeric: 'tabular-nums',
})

const completedTagStyle = css({
  fontSize: 'xs',
  color: 'accent',
  fontWeight: 'semibold',
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

interface ContractProgressBarProps {
  done: number
  total: number
  isCompleted: boolean
}

export function ContractProgressBar({
  done,
  total,
  isCompleted,
}: ContractProgressBarProps) {
  const { t } = useTranslation()
  const percent = total === 0 ? 0 : Math.round((done / total) * 100)
  return (
    <div className={wrapperStyle}>
      <div className={labelRowStyle}>
        <span>
          {t('dashboard.contracts.progress', { done, total })}
          {isCompleted ? (
            <>
              {' '}
              <span className={completedTagStyle}>
                ({t('contracts.detail.completed')})
              </span>
            </>
          ) : null}
        </span>
        <span>{percent}%</span>
      </div>
      <div
        className={barStyle}
        role="progressbar"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div className={cx(fillStyle, pickFillWidthBucket(percent))} />
      </div>
    </div>
  )
}
