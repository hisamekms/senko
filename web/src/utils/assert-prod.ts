import { isLikelyArn } from '#/utils/secrets/resolve'

const SECRET_NAMES = ['AUTH_SECRET', 'AUTH_OIDC_CLIENT_SECRET'] as const

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

  for (const name of SECRET_NAMES) {
    const raw = process.env[name]
    const arn = process.env[`${name}_ARN`]
    const hasRaw = typeof raw === 'string' && raw.length > 0
    const hasArn = typeof arn === 'string' && arn.length > 0

    if (!hasRaw && !hasArn) {
      errors.push(
        `${name} is required in production (set ${name} or ${name}_ARN).`,
      )
    }
    if (hasArn && !isLikelyArn(arn)) {
      errors.push(
        `${name}_ARN does not look like a Secrets Manager ARN (got: ${arn}). ` +
          'Expected format: arn:aws:secretsmanager:<region>:<account-id>:secret:<name>.',
      )
    }
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
