import { Switch } from '@ark-ui/react/switch'
import { useTranslation } from 'react-i18next'

import { useTheme } from '#/hooks/useTheme'
import { css } from '../../styled-system/css'

const labelStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '2',
  cursor: 'pointer',
  userSelect: 'none',
})

const controlStyle = css({
  position: 'relative',
  width: '40px',
  height: '22px',
  borderRadius: 'full',
  backgroundColor: 'border',
  transition: 'background-color 120ms ease',
  _checked: {
    backgroundColor: 'accent',
  },
})

const thumbStyle = css({
  position: 'absolute',
  top: '2px',
  left: '2px',
  width: '18px',
  height: '18px',
  borderRadius: 'full',
  backgroundColor: 'bg',
  boxShadow: '0 1px 2px rgba(0, 0, 0, 0.3)',
  transition: 'transform 120ms ease',
  _checked: {
    transform: 'translateX(18px)',
  },
})

export function ThemeToggle() {
  const { t } = useTranslation()
  const { theme, setTheme } = useTheme()
  const isDark = theme === 'dark'

  return (
    <Switch.Root
      checked={isDark}
      onCheckedChange={(details) => setTheme(details.checked ? 'dark' : 'light')}
      className={labelStyle}
      aria-label={t('theme.toggle')}
    >
      <Switch.Control className={controlStyle}>
        <Switch.Thumb className={thumbStyle} />
      </Switch.Control>
      <Switch.Label>
        {isDark ? t('theme.dark') : t('theme.light')}
      </Switch.Label>
      <Switch.HiddenInput />
    </Switch.Root>
  )
}
