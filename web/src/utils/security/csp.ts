export const REQUEST_NONCE_GLOBAL_KEY = Symbol.for(
  'senko-web:request-csp-nonce',
)

type GlobalWithNonce = {
  [REQUEST_NONCE_GLOBAL_KEY]?: () => string | undefined
}

export function readCurrentRequestNonce(): string | undefined {
  if (typeof globalThis === 'undefined') return undefined
  const getter = (globalThis as GlobalWithNonce)[REQUEST_NONCE_GLOBAL_KEY]
  return getter?.()
}

export function generateNonce(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  let bin = ''
  for (let i = 0; i < bytes.length; i += 1) bin += String.fromCharCode(bytes[i])
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export const CSP_EXTRA_DIRECTIVES = [
  'connect-src',
  'img-src',
  'script-src',
  'style-src',
  'font-src',
] as const

export type CspExtraDirective = (typeof CSP_EXTRA_DIRECTIVES)[number]

export type CspExtraOrigins = Partial<Record<CspExtraDirective, string[]>>

export interface SecurityHeadersEnvOptions {
  cspReportOnly?: boolean
  cspReportUri?: string
  cspExtra?: CspExtraOrigins
  hstsDisabled?: boolean
  hstsMaxAge?: number
  hstsPreload?: boolean
  coopDisabled?: boolean
  corpDisabled?: boolean
}

export interface SecurityHeadersOptions extends SecurityHeadersEnvOptions {
  nonce: string
  isDev: boolean
}

export type CspOptions = Pick<
  SecurityHeadersOptions,
  'nonce' | 'isDev' | 'cspReportUri' | 'cspExtra'
>

const HSTS_DEFAULT_MAX_AGE = 31_536_000

const PERMISSIONS_POLICY = [
  'camera=()',
  'microphone=()',
  'geolocation=()',
  'payment=()',
  'usb=()',
  'midi=()',
  'serial=()',
  'bluetooth=()',
  'magnetometer=()',
  'accelerometer=()',
  'gyroscope=()',
  'interest-cohort=()',
].join(', ')

function sanitizeCspToken(input: string): string {
  return input.replace(/[\s;,]+/g, '')
}

function sanitizeReportUri(input: string): string {
  return input.replace(/[\s;]+/g, '').trim()
}

function parseExtraList(raw: string | undefined): string[] | undefined {
  if (!raw) return undefined
  const tokens = raw
    .split(/[\s,]+/)
    .map(sanitizeCspToken)
    .filter((s) => s.length > 0)
  return tokens.length > 0 ? tokens : undefined
}

function appendExtras(
  directive: string,
  extras: string[] | undefined,
): string {
  if (!extras || extras.length === 0) return directive
  return `${directive} ${extras.join(' ')}`
}

export function buildCspHeader({
  nonce,
  isDev,
  cspReportUri,
  cspExtra,
}: CspOptions): string {
  const scriptSrc = appendExtras(
    isDev
      ? `script-src 'self' 'nonce-${nonce}' 'unsafe-eval'`
      : `script-src 'self' 'nonce-${nonce}'`,
    cspExtra?.['script-src'],
  )
  const connectSrc = appendExtras(
    isDev ? "connect-src 'self' ws: wss:" : "connect-src 'self'",
    cspExtra?.['connect-src'],
  )
  const styleSrc = appendExtras(
    "style-src 'self'",
    cspExtra?.['style-src'],
  )
  const imgSrc = appendExtras("img-src 'self' data:", cspExtra?.['img-src'])
  const fontSrc = appendExtras("font-src 'self' data:", cspExtra?.['font-src'])

  // Allow ark-ui inline `style="..."` attributes (visually-hidden inputs,
  // Floating UI portal positioning) without weakening protection of
  // <style>/<link> elements. style-src-elem is intentionally NOT emitted,
  // so element-style sinks fall back to `style-src 'self'`.
  const directives = [
    "default-src 'self'",
    scriptSrc,
    styleSrc,
    "style-src-attr 'unsafe-inline'",
    imgSrc,
    fontSrc,
    connectSrc,
    "frame-ancestors 'none'",
    "base-uri 'self'",
    "object-src 'none'",
    "form-action 'self'",
  ]
  if (cspReportUri) {
    const safe = sanitizeReportUri(cspReportUri)
    if (safe) directives.push(`report-uri ${safe}`)
  }
  return directives.join('; ')
}

export function buildSecurityHeaders(
  options: SecurityHeadersOptions,
): Record<string, string> {
  const {
    nonce,
    isDev,
    cspReportOnly,
    cspReportUri,
    cspExtra,
    hstsDisabled,
    hstsMaxAge,
    hstsPreload,
    coopDisabled,
    corpDisabled,
  } = options
  const csp = buildCspHeader({ nonce, isDev, cspReportUri, cspExtra })
  const headers: Record<string, string> = {
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    'X-Content-Type-Options': 'nosniff',
    'Permissions-Policy': PERMISSIONS_POLICY,
  }
  if (!coopDisabled) headers['Cross-Origin-Opener-Policy'] = 'same-origin'
  if (!corpDisabled) headers['Cross-Origin-Resource-Policy'] = 'same-origin'

  const useReportOnly = isDev || Boolean(cspReportOnly)
  if (useReportOnly) {
    headers['Content-Security-Policy-Report-Only'] = csp
  } else {
    headers['Content-Security-Policy'] = csp
  }

  if (!isDev && !hstsDisabled) {
    const maxAge =
      typeof hstsMaxAge === 'number' && Number.isFinite(hstsMaxAge) && hstsMaxAge >= 0
        ? Math.floor(hstsMaxAge)
        : HSTS_DEFAULT_MAX_AGE
    const suffix = hstsPreload ? '; preload' : ''
    headers['Strict-Transport-Security'] = `max-age=${maxAge}; includeSubDomains${suffix}`
  }

  return headers
}

function readBool(env: NodeJS.ProcessEnv, name: string): boolean | undefined {
  const v = env[name]
  if (v === undefined) return undefined
  return v === 'true'
}

function readNumber(
  env: NodeJS.ProcessEnv,
  name: string,
): number | undefined {
  const v = env[name]
  if (v === undefined || v === '') return undefined
  const n = Number(v)
  if (!Number.isFinite(n) || n < 0) return undefined
  return n
}

export function parseSecurityHeadersEnv(
  env: NodeJS.ProcessEnv,
): SecurityHeadersEnvOptions {
  const cspExtraEntries: [CspExtraDirective, string[]][] = []
  const extraEnvMap: Record<CspExtraDirective, string> = {
    'connect-src': 'CSP_EXTRA_CONNECT_SRC',
    'img-src': 'CSP_EXTRA_IMG_SRC',
    'script-src': 'CSP_EXTRA_SCRIPT_SRC',
    'style-src': 'CSP_EXTRA_STYLE_SRC',
    'font-src': 'CSP_EXTRA_FONT_SRC',
  }
  for (const directive of CSP_EXTRA_DIRECTIVES) {
    const tokens = parseExtraList(env[extraEnvMap[directive]])
    if (tokens) cspExtraEntries.push([directive, tokens])
  }
  const cspExtra =
    cspExtraEntries.length > 0
      ? (Object.fromEntries(cspExtraEntries) as CspExtraOrigins)
      : undefined

  const reportUriRaw = env.CSP_REPORT_URI
  let cspReportUri: string | undefined
  if (reportUriRaw) {
    const safe = sanitizeReportUri(reportUriRaw)
    if (safe) cspReportUri = safe
  }

  return {
    cspReportOnly: readBool(env, 'CSP_REPORT_ONLY'),
    cspReportUri,
    cspExtra,
    hstsDisabled: readBool(env, 'HSTS_DISABLED'),
    hstsMaxAge: readNumber(env, 'HSTS_MAX_AGE'),
    hstsPreload: readBool(env, 'HSTS_PRELOAD'),
    coopDisabled: readBool(env, 'COOP_DISABLED'),
    corpDisabled: readBool(env, 'CORP_DISABLED'),
  }
}
