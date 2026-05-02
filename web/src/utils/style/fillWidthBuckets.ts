import { css } from '../../../styled-system/css'

// 21 buckets in 5% steps (0%, 5%, 10%, ..., 100%). Pre-extracted by Panda's
// static analyzer so the dynamic-percent progress fills (ContractProgressBar,
// ContractsCard) can pick a className from the array instead of using a
// `style="width: N%"` attribute that would violate `style-src 'self'`.
export const FILL_WIDTH_BUCKETS = [
  css({ width: '0%' }),
  css({ width: '5%' }),
  css({ width: '10%' }),
  css({ width: '15%' }),
  css({ width: '20%' }),
  css({ width: '25%' }),
  css({ width: '30%' }),
  css({ width: '35%' }),
  css({ width: '40%' }),
  css({ width: '45%' }),
  css({ width: '50%' }),
  css({ width: '55%' }),
  css({ width: '60%' }),
  css({ width: '65%' }),
  css({ width: '70%' }),
  css({ width: '75%' }),
  css({ width: '80%' }),
  css({ width: '85%' }),
  css({ width: '90%' }),
  css({ width: '95%' }),
  css({ width: '100%' }),
] as const

export function pickFillWidthBucket(percent: number): string {
  const clamped = Math.max(0, Math.min(100, percent))
  return FILL_WIDTH_BUCKETS[Math.round(clamped / 5)]
}
