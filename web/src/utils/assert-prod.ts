export function assertProductionConfig(): void {
  if (process.env.NODE_ENV !== 'production') return

  const errors: string[] = []

  if (process.env.WEB_DEV_AUTH_BYPASS === 'true') {
    errors.push(
      'WEB_DEV_AUTH_BYPASS=true is forbidden in production ' +
        '(unset it or set it to "false").',
    )
  }

  const authUrl = process.env.AUTH_URL
  if (authUrl && !authUrl.startsWith('https://')) {
    errors.push(
      `AUTH_URL must use https:// in production (got: ${authUrl}). ` +
        'HTTP disables Secure cookies and the __Secure-/__Host- prefix.',
    )
  }

  const issuer = process.env.AUTH_OIDC_ISSUER
  if (issuer && !issuer.startsWith('https://')) {
    errors.push(
      `AUTH_OIDC_ISSUER must use https:// in production (got: ${issuer}). ` +
        'OIDC discovery/token/userinfo over HTTP is not RFC 9700 compliant.',
    )
  }

  if (errors.length > 0) {
    throw new Error(
      'Refusing to start (production config check failed):\n  - ' +
        errors.join('\n  - '),
    )
  }
}

if (typeof window === 'undefined') {
  assertProductionConfig()
}
