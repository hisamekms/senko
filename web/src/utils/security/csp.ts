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

export interface CspOptions {
  nonce: string
  isDev: boolean
}

export function buildCspHeader({ nonce, isDev }: CspOptions): string {
  const scriptSrc = isDev
    ? `script-src 'self' 'nonce-${nonce}' 'unsafe-eval'`
    : `script-src 'self' 'nonce-${nonce}'`
  const connectSrc = isDev
    ? "connect-src 'self' ws: wss:"
    : "connect-src 'self'"
  return [
    "default-src 'self'",
    scriptSrc,
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self' data:",
    connectSrc,
    "frame-ancestors 'none'",
    "base-uri 'self'",
    "object-src 'none'",
    "form-action 'self'",
  ].join('; ')
}

export function buildSecurityHeaders({
  nonce,
  isDev,
}: CspOptions): Record<string, string> {
  const csp = buildCspHeader({ nonce, isDev })
  const headers: Record<string, string> = {
    'X-Frame-Options': 'DENY',
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    'X-Content-Type-Options': 'nosniff',
    'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
  }
  if (isDev) {
    headers['Content-Security-Policy-Report-Only'] = csp
  } else {
    headers['Content-Security-Policy'] = csp
    headers['Strict-Transport-Security'] = 'max-age=31536000; includeSubDomains'
  }
  return headers
}
