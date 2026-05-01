import { useTranslation } from 'react-i18next'

import { LanguageSwitcher } from '#/components/LanguageSwitcher'
import { ProjectSwitcher } from '#/components/ProjectSwitcher'
import { ThemeToggle } from '#/components/ThemeToggle'
import { css } from '../../styled-system/css'

const headerStyle = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  paddingX: '6',
  paddingY: '4',
  borderBottom: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  gap: '4',
  flexWrap: 'wrap',
})

const leftStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '4',
  flexWrap: 'wrap',
})

const rightStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '4',
  flexWrap: 'wrap',
})

const titleStyle = css({
  fontSize: 'lg',
  fontWeight: 'semibold',
  color: 'fg',
})

const userInfoStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.8',
})

const signOutStyle = css({
  fontSize: 'sm',
  fontWeight: 'medium',
  color: 'accent',
  textDecoration: 'underline',
  _hover: { opacity: '0.8' },
})

interface AppHeaderProps {
  currentProjectId: number | null
  userLabel?: string | null
}

export function AppHeader({ currentProjectId, userLabel }: AppHeaderProps) {
  const { t } = useTranslation()

  return (
    <header className={headerStyle}>
      <div className={leftStyle}>
        <span className={titleStyle}>{t('app.title')}</span>
        <ProjectSwitcher currentProjectId={currentProjectId} />
      </div>
      <div className={rightStyle}>
        {userLabel ? <span className={userInfoStyle}>{userLabel}</span> : null}
        <a href="/api/auth/signout" className={signOutStyle}>
          {t('auth.signOut')}
        </a>
        <LanguageSwitcher />
        <ThemeToggle />
      </div>
    </header>
  )
}
