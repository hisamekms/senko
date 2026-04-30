import { createFileRoute } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { LanguageSwitcher } from '#/components/LanguageSwitcher'
import { ThemeToggle } from '#/components/ThemeToggle'
import { useTheme } from '#/hooks/useTheme'
import { css } from '../../../styled-system/css'

export const Route = createFileRoute('/_authed/')({ component: Home })

const pageStyle = css({
  minHeight: '100vh',
  display: 'flex',
  flexDirection: 'column',
})

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

const headerActionsStyle = css({
  display: 'inline-flex',
  alignItems: 'center',
  gap: '4',
  flexWrap: 'wrap',
})

const headerTitleStyle = css({
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

const mainStyle = css({
  flex: '1',
  paddingX: '6',
  paddingY: '8',
  display: 'flex',
  flexDirection: 'column',
  gap: '4',
  maxWidth: '720px',
  margin: '0 auto',
  width: '100%',
})

const titleStyle = css({
  fontSize: '3xl',
  fontWeight: 'bold',
  color: 'fg',
})

const subtitleStyle = css({
  fontSize: 'md',
  color: 'fg',
  opacity: '0.8',
})

const taglineStyle = css({
  fontSize: 'sm',
  color: 'accent',
  fontWeight: 'medium',
})

const calloutStyle = css({
  padding: '4',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  color: 'fg',
})

const metaStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.7',
  display: 'flex',
  flexDirection: 'column',
  gap: '1',
})

function Home() {
  const { t, i18n } = useTranslation()
  const { theme, hydrated } = useTheme()
  const { session } = Route.useRouteContext()
  const language = i18n.resolvedLanguage ?? i18n.language ?? 'en'
  const userLabel = session?.user?.name ?? session?.user?.email ?? ''

  return (
    <div className={pageStyle}>
      <header className={headerStyle}>
        <span className={headerTitleStyle}>senko Web</span>
        <div className={headerActionsStyle}>
          {userLabel ? <span className={userInfoStyle}>{userLabel}</span> : null}
          <a href="/api/auth/signout" className={signOutStyle}>
            {t('auth.signOut')}
          </a>
          <LanguageSwitcher />
          <ThemeToggle />
        </div>
      </header>
      <main className={mainStyle}>
        <h1 className={titleStyle}>{t('app.title')}</h1>
        <p className={subtitleStyle}>{t('app.subtitle')}</p>
        <p className={taglineStyle}>{t('app.tagline')}</p>
        <div className={calloutStyle}>{t('skeleton.deferred')}</div>
        <div className={metaStyle}>
          <span suppressHydrationWarning>
            {t('skeleton.current_theme', {
              theme: hydrated ? t(`theme.${theme}`) : '…',
            })}
          </span>
          <span suppressHydrationWarning>
            {t('skeleton.current_language', {
              language: t(`language.${language}`),
            })}
          </span>
        </div>
      </main>
    </div>
  )
}
