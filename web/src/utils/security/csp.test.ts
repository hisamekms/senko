import { describe, it, expect } from 'vitest'

import { buildCspHeader, buildSecurityHeaders } from './csp'

const NONCE = 'test-nonce-AbCdEf123456'

const PERMISSIONS_POLICY_FEATURES = [
  'camera',
  'microphone',
  'geolocation',
  'payment',
  'usb',
  'midi',
  'serial',
  'bluetooth',
  'magnetometer',
  'accelerometer',
  'gyroscope',
  'interest-cohort',
]

describe('buildSecurityHeaders', () => {
  describe('Permissions-Policy', () => {
    it.each(PERMISSIONS_POLICY_FEATURES)(
      'denies %s with empty allowlist (dev)',
      (feature) => {
        const headers = buildSecurityHeaders({ nonce: NONCE, isDev: true })
        expect(headers['Permissions-Policy']).toContain(`${feature}=()`)
      },
    )

    it.each(PERMISSIONS_POLICY_FEATURES)(
      'denies %s with empty allowlist (prod)',
      (feature) => {
        const headers = buildSecurityHeaders({ nonce: NONCE, isDev: false })
        expect(headers['Permissions-Policy']).toContain(`${feature}=()`)
      },
    )
  })

  describe('Cross-Origin headers', () => {
    it('sets COOP to same-origin in dev', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: true })
      expect(headers['Cross-Origin-Opener-Policy']).toBe('same-origin')
    })

    it('sets COOP to same-origin in prod', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: false })
      expect(headers['Cross-Origin-Opener-Policy']).toBe('same-origin')
    })

    it('sets CORP to same-origin in dev', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: true })
      expect(headers['Cross-Origin-Resource-Policy']).toBe('same-origin')
    })

    it('sets CORP to same-origin in prod', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: false })
      expect(headers['Cross-Origin-Resource-Policy']).toBe('same-origin')
    })
  })

  describe('environment-dependent headers', () => {
    it('emits Report-Only CSP and no enforcing CSP / HSTS in dev', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: true })
      expect(headers['Content-Security-Policy-Report-Only']).toBeTruthy()
      expect(headers['Content-Security-Policy']).toBeUndefined()
      expect(headers['Strict-Transport-Security']).toBeUndefined()
    })

    it('emits enforcing CSP and HSTS but no Report-Only in prod', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: false })
      expect(headers['Content-Security-Policy']).toBeTruthy()
      expect(headers['Strict-Transport-Security']).toBe(
        'max-age=31536000; includeSubDomains',
      )
      expect(headers['Content-Security-Policy-Report-Only']).toBeUndefined()
    })
  })

  describe('baseline static headers', () => {
    it('sets Referrer-Policy and X-Content-Type-Options', () => {
      const headers = buildSecurityHeaders({ nonce: NONCE, isDev: true })
      expect(headers['Referrer-Policy']).toBe('strict-origin-when-cross-origin')
      expect(headers['X-Content-Type-Options']).toBe('nosniff')
    })

    it('does not set X-Frame-Options (covered by CSP frame-ancestors)', () => {
      const dev = buildSecurityHeaders({ nonce: NONCE, isDev: true })
      const prod = buildSecurityHeaders({ nonce: NONCE, isDev: false })
      expect(dev['X-Frame-Options']).toBeUndefined()
      expect(prod['X-Frame-Options']).toBeUndefined()
    })
  })
})

describe('buildCspHeader', () => {
  it('embeds the nonce in script-src and includes unsafe-eval in dev', () => {
    const csp = buildCspHeader({ nonce: NONCE, isDev: true })
    expect(csp).toContain(`script-src 'self' 'nonce-${NONCE}' 'unsafe-eval'`)
    expect(csp).toContain("connect-src 'self' ws: wss:")
  })

  it('embeds the nonce in script-src without unsafe-eval in prod', () => {
    const csp = buildCspHeader({ nonce: NONCE, isDev: false })
    expect(csp).toContain(`script-src 'self' 'nonce-${NONCE}'`)
    expect(csp).not.toContain("'unsafe-eval'")
    expect(csp).toContain("connect-src 'self'")
    expect(csp).not.toContain('ws:')
  })

  it('locks down framing, base-uri, object-src, and form-action', () => {
    const csp = buildCspHeader({ nonce: NONCE, isDev: false })
    expect(csp).toContain("frame-ancestors 'none'")
    expect(csp).toContain("base-uri 'self'")
    expect(csp).toContain("object-src 'none'")
    expect(csp).toContain("form-action 'self'")
    expect(csp).toContain("default-src 'self'")
  })
})
