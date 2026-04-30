import { useEffect, useState } from 'react'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

import { css } from '../../styled-system/css'

export const Route = createFileRoute('/login')({
  beforeLoad: ({ context }) => {
    if (context.session) {
      throw redirect({ to: '/' })
    }
  },
  component: Login,
})

const pageStyle = css({
  minHeight: '100vh',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  paddingX: '6',
  paddingY: '8',
})

const cardStyle = css({
  width: '100%',
  maxWidth: '24rem',
  padding: '6',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'border',
  backgroundColor: 'surface',
  display: 'flex',
  flexDirection: 'column',
  gap: '4',
})

const titleStyle = css({
  fontSize: 'xl',
  fontWeight: 'semibold',
  color: 'fg',
})

const noticeStyle = css({
  fontSize: 'sm',
  color: 'fg',
  opacity: '0.75',
})

const buttonStyle = css({
  paddingX: '4',
  paddingY: '3',
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'accent',
  backgroundColor: 'accent',
  color: 'surface',
  fontWeight: 'medium',
  cursor: 'pointer',
  _hover: { opacity: '0.9' },
})

function Login() {
  const { t } = useTranslation()
  const [csrfToken, setCsrfToken] = useState('')

  useEffect(() => {
    let aborted = false
    fetch('/api/auth/csrf')
      .then((res) => res.json())
      .then((data: { csrfToken?: string }) => {
        if (!aborted && data.csrfToken) setCsrfToken(data.csrfToken)
      })
      .catch(() => {
        /* leave empty; submit will fail visibly */
      })
    return () => {
      aborted = true
    }
  }, [])

  return (
    <div className={pageStyle}>
      <form
        action="/api/auth/signin/oidc"
        method="POST"
        className={cardStyle}
      >
        <h1 className={titleStyle}>{t('auth.loginTitle')}</h1>
        <p className={noticeStyle}>{t('auth.loginRedirectNotice')}</p>
        <input type="hidden" name="csrfToken" value={csrfToken} />
        <input type="hidden" name="callbackUrl" value="/" />
        <button type="submit" className={buttonStyle} disabled={!csrfToken}>
          {t('auth.signIn')}
        </button>
      </form>
    </div>
  )
}
