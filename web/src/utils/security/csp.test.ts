import { describe, it, expect } from 'vitest'

import {
  buildCspHeader,
  buildSecurityHeaders,
  parseSecurityHeadersEnv,
  type SecurityHeadersOptions,
} from './csp'

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

const baseOpts = (
  overrides: Partial<SecurityHeadersOptions> = {},
): SecurityHeadersOptions => ({
  nonce: NONCE,
  isDev: false,
  ...overrides,
})

describe('buildSecurityHeaders (defaults)', () => {
  describe('Permissions-Policy', () => {
    it.each(PERMISSIONS_POLICY_FEATURES)(
      'denies %s with empty allowlist (dev)',
      (feature) => {
        const headers = buildSecurityHeaders(baseOpts({ isDev: true }))
        expect(headers['Permissions-Policy']).toContain(`${feature}=()`)
      },
    )

    it.each(PERMISSIONS_POLICY_FEATURES)(
      'denies %s with empty allowlist (prod)',
      (feature) => {
        const headers = buildSecurityHeaders(baseOpts({ isDev: false }))
        expect(headers['Permissions-Policy']).toContain(`${feature}=()`)
      },
    )
  })

  describe('Cross-Origin headers', () => {
    it('sets COOP to same-origin in dev', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: true }))
      expect(headers['Cross-Origin-Opener-Policy']).toBe('same-origin')
    })

    it('sets COOP to same-origin in prod', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: false }))
      expect(headers['Cross-Origin-Opener-Policy']).toBe('same-origin')
    })

    it('sets CORP to same-origin in dev', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: true }))
      expect(headers['Cross-Origin-Resource-Policy']).toBe('same-origin')
    })

    it('sets CORP to same-origin in prod', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: false }))
      expect(headers['Cross-Origin-Resource-Policy']).toBe('same-origin')
    })
  })

  describe('environment-dependent headers', () => {
    it('emits Report-Only CSP and no enforcing CSP / HSTS in dev', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: true }))
      expect(headers['Content-Security-Policy-Report-Only']).toBeTruthy()
      expect(headers['Content-Security-Policy']).toBeUndefined()
      expect(headers['Strict-Transport-Security']).toBeUndefined()
    })

    it('emits enforcing CSP and HSTS but no Report-Only in prod', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: false }))
      expect(headers['Content-Security-Policy']).toBeTruthy()
      expect(headers['Strict-Transport-Security']).toBe(
        'max-age=31536000; includeSubDomains',
      )
      expect(headers['Content-Security-Policy-Report-Only']).toBeUndefined()
    })
  })

  describe('baseline static headers', () => {
    it('sets Referrer-Policy and X-Content-Type-Options', () => {
      const headers = buildSecurityHeaders(baseOpts({ isDev: true }))
      expect(headers['Referrer-Policy']).toBe('strict-origin-when-cross-origin')
      expect(headers['X-Content-Type-Options']).toBe('nosniff')
    })

    it('does not set X-Frame-Options (covered by CSP frame-ancestors)', () => {
      const dev = buildSecurityHeaders(baseOpts({ isDev: true }))
      const prod = buildSecurityHeaders(baseOpts({ isDev: false }))
      expect(dev['X-Frame-Options']).toBeUndefined()
      expect(prod['X-Frame-Options']).toBeUndefined()
    })
  })
})

describe('buildCspHeader (defaults)', () => {
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

  // style-src must NOT contain 'unsafe-inline' — JSX `style={...}` attributes
  // and any other inline-style sinks are expected to be expressed as classes
  // (Panda CSS) instead, so the directive can be locked to 'self' only.
  it.each([true, false])(
    'pins style-src to a single source-expression and excludes unsafe-inline (isDev=%s)',
    (isDev) => {
      const csp = buildCspHeader({ nonce: NONCE, isDev })
      const styleSrcMatch = csp.match(/style-src ([^;]+)/)
      expect(styleSrcMatch).not.toBeNull()
      const styleSrcValue = styleSrcMatch![1].trim()
      expect(styleSrcValue).toBe("'self'")
      expect(styleSrcValue).not.toContain("'unsafe-inline'")
    },
  )
})

describe('CSP_REPORT_ONLY', () => {
  it('replaces enforcing CSP with Report-Only in prod when true', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ isDev: false, cspReportOnly: true }),
    )
    expect(headers['Content-Security-Policy']).toBeUndefined()
    expect(headers['Content-Security-Policy-Report-Only']).toBeTruthy()
  })

  it('keeps enforcing CSP in prod when omitted or false', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ isDev: false, cspReportOnly: false }),
    )
    expect(headers['Content-Security-Policy']).toBeTruthy()
    expect(headers['Content-Security-Policy-Report-Only']).toBeUndefined()
  })

  it('does not break dev (already Report-Only)', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ isDev: true, cspReportOnly: true }),
    )
    expect(headers['Content-Security-Policy-Report-Only']).toBeTruthy()
    expect(headers['Content-Security-Policy']).toBeUndefined()
  })
})

describe('CSP_REPORT_URI', () => {
  it('appends report-uri directive at end of CSP', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspReportUri: 'https://reports.example.com/csp',
    })
    expect(csp.endsWith('report-uri https://reports.example.com/csp')).toBe(
      true,
    )
  })

  it('flows through buildSecurityHeaders enforcing CSP', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ cspReportUri: 'https://r.example.com/csp' }),
    )
    expect(headers['Content-Security-Policy']).toContain(
      'report-uri https://r.example.com/csp',
    )
  })

  it('omits report-uri when not set', () => {
    const csp = buildCspHeader({ nonce: NONCE, isDev: false })
    expect(csp).not.toContain('report-uri')
  })
})

describe('CSP_EXTRA_* (additional origins)', () => {
  it('appends extra origins to connect-src after defaults', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspExtra: { 'connect-src': ['https://api.example.com'] },
    })
    expect(csp).toContain("connect-src 'self' https://api.example.com")
  })

  it('appends extra origins to script-src after the nonce', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspExtra: { 'script-src': ['https://cdn.example.com'] },
    })
    expect(csp).toContain(
      `script-src 'self' 'nonce-${NONCE}' https://cdn.example.com`,
    )
  })

  it('supports img-src, style-src, and font-src extras', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspExtra: {
        'img-src': ['https://images.example.com'],
        'style-src': ['https://styles.example.com'],
        'font-src': ['https://fonts.example.com'],
      },
    })
    expect(csp).toContain(
      "img-src 'self' data: https://images.example.com",
    )
    expect(csp).toContain(
      "style-src 'self' https://styles.example.com",
    )
    expect(csp).toContain(
      "font-src 'self' data: https://fonts.example.com",
    )
  })

  it('appends extras to dev connect-src after ws:/wss:', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: true,
      cspExtra: { 'connect-src': ['https://api.example.com'] },
    })
    expect(csp).toContain(
      "connect-src 'self' ws: wss: https://api.example.com",
    )
  })
})

describe('HSTS env handling', () => {
  it('omits HSTS in prod when hstsDisabled=true', () => {
    const headers = buildSecurityHeaders(baseOpts({ hstsDisabled: true }))
    expect(headers['Strict-Transport-Security']).toBeUndefined()
  })

  it('honors hstsMaxAge override', () => {
    const headers = buildSecurityHeaders(baseOpts({ hstsMaxAge: 63072000 }))
    expect(headers['Strict-Transport-Security']).toBe(
      'max-age=63072000; includeSubDomains',
    )
  })

  it('appends ; preload when hstsPreload=true', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ hstsMaxAge: 63072000, hstsPreload: true }),
    )
    expect(headers['Strict-Transport-Security']).toBe(
      'max-age=63072000; includeSubDomains; preload',
    )
  })

  it('uses default max-age when hstsMaxAge is invalid', () => {
    const headers = buildSecurityHeaders(baseOpts({ hstsMaxAge: -1 }))
    expect(headers['Strict-Transport-Security']).toBe(
      'max-age=31536000; includeSubDomains',
    )
  })

  it('does not emit HSTS in dev even with hstsPreload=true', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ isDev: true, hstsPreload: true, hstsMaxAge: 63072000 }),
    )
    expect(headers['Strict-Transport-Security']).toBeUndefined()
  })
})

describe('COOP_DISABLED / CORP_DISABLED', () => {
  it('omits COOP when coopDisabled=true', () => {
    const headers = buildSecurityHeaders(baseOpts({ coopDisabled: true }))
    expect(headers['Cross-Origin-Opener-Policy']).toBeUndefined()
    expect(headers['Cross-Origin-Resource-Policy']).toBe('same-origin')
  })

  it('omits CORP when corpDisabled=true', () => {
    const headers = buildSecurityHeaders(baseOpts({ corpDisabled: true }))
    expect(headers['Cross-Origin-Resource-Policy']).toBeUndefined()
    expect(headers['Cross-Origin-Opener-Policy']).toBe('same-origin')
  })

  it('omits both when both disabled', () => {
    const headers = buildSecurityHeaders(
      baseOpts({ coopDisabled: true, corpDisabled: true }),
    )
    expect(headers['Cross-Origin-Opener-Policy']).toBeUndefined()
    expect(headers['Cross-Origin-Resource-Policy']).toBeUndefined()
  })
})

describe('header injection sanitization', () => {
  it('strips ; and other CSP separators from extra-origin tokens', () => {
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspExtra: {
        'connect-src': parseSecurityHeadersEnv({
          CSP_EXTRA_CONNECT_SRC: "https://evil.com; script-src 'unsafe-inline'",
        }).cspExtra?.['connect-src'],
      },
    })
    // Each token sanitized — `;` removed, internal whitespace dropped.
    expect(csp).not.toMatch(/connect-src[^;]*; script-src/)
    expect(csp).toContain(
      "connect-src 'self' https://evil.com script-src",
    )
  })

  it("strips \\r\\n from CSP_REPORT_URI", () => {
    const opts = parseSecurityHeadersEnv({
      CSP_REPORT_URI: 'https://r.example.com/csp\r\nX-Injected: 1',
    })
    expect(opts.cspReportUri).toBe(
      'https://r.example.com/cspX-Injected:1',
    )
    const csp = buildCspHeader({
      nonce: NONCE,
      isDev: false,
      cspReportUri: opts.cspReportUri,
    })
    expect(csp).not.toContain('\n')
    expect(csp).not.toContain('\r')
  })

  it('drops empty tokens from CSP_EXTRA_*', () => {
    const opts = parseSecurityHeadersEnv({
      CSP_EXTRA_IMG_SRC: ' , ,  ,',
    })
    expect(opts.cspExtra).toBeUndefined()
  })
})

describe('parseSecurityHeadersEnv', () => {
  it('returns all undefined for empty env', () => {
    const opts = parseSecurityHeadersEnv({})
    expect(opts.cspReportOnly).toBeUndefined()
    expect(opts.cspReportUri).toBeUndefined()
    expect(opts.cspExtra).toBeUndefined()
    expect(opts.hstsDisabled).toBeUndefined()
    expect(opts.hstsMaxAge).toBeUndefined()
    expect(opts.hstsPreload).toBeUndefined()
    expect(opts.coopDisabled).toBeUndefined()
    expect(opts.corpDisabled).toBeUndefined()
  })

  it('parses booleans as true only for the literal "true"', () => {
    const yes = parseSecurityHeadersEnv({
      CSP_REPORT_ONLY: 'true',
      HSTS_DISABLED: 'true',
      HSTS_PRELOAD: 'true',
      COOP_DISABLED: 'true',
      CORP_DISABLED: 'true',
    })
    expect(yes.cspReportOnly).toBe(true)
    expect(yes.hstsDisabled).toBe(true)
    expect(yes.hstsPreload).toBe(true)
    expect(yes.coopDisabled).toBe(true)
    expect(yes.corpDisabled).toBe(true)

    const no = parseSecurityHeadersEnv({
      CSP_REPORT_ONLY: '1',
      HSTS_DISABLED: 'yes',
      HSTS_PRELOAD: 'TRUE',
      COOP_DISABLED: '',
    })
    expect(no.cspReportOnly).toBe(false)
    expect(no.hstsDisabled).toBe(false)
    expect(no.hstsPreload).toBe(false)
    expect(no.coopDisabled).toBe(false)
  })

  it('parses HSTS_MAX_AGE as a non-negative number, ignoring invalid values', () => {
    expect(parseSecurityHeadersEnv({ HSTS_MAX_AGE: '63072000' }).hstsMaxAge).toBe(
      63072000,
    )
    expect(parseSecurityHeadersEnv({ HSTS_MAX_AGE: '0' }).hstsMaxAge).toBe(0)
    expect(parseSecurityHeadersEnv({ HSTS_MAX_AGE: '-5' }).hstsMaxAge).toBeUndefined()
    expect(parseSecurityHeadersEnv({ HSTS_MAX_AGE: 'abc' }).hstsMaxAge).toBeUndefined()
    expect(parseSecurityHeadersEnv({ HSTS_MAX_AGE: '' }).hstsMaxAge).toBeUndefined()
  })

  it('splits CSP_EXTRA_* on commas and whitespace and sanitizes tokens', () => {
    const opts = parseSecurityHeadersEnv({
      CSP_EXTRA_CONNECT_SRC: 'https://a.example.com, https://b.example.com',
      CSP_EXTRA_IMG_SRC: 'https://c.example.com\thttps://d.example.com',
    })
    expect(opts.cspExtra?.['connect-src']).toEqual([
      'https://a.example.com',
      'https://b.example.com',
    ])
    expect(opts.cspExtra?.['img-src']).toEqual([
      'https://c.example.com',
      'https://d.example.com',
    ])
  })

  it('maps all five CSP_EXTRA_* env keys', () => {
    const opts = parseSecurityHeadersEnv({
      CSP_EXTRA_CONNECT_SRC: 'https://a.example.com',
      CSP_EXTRA_IMG_SRC: 'https://b.example.com',
      CSP_EXTRA_SCRIPT_SRC: 'https://c.example.com',
      CSP_EXTRA_STYLE_SRC: 'https://d.example.com',
      CSP_EXTRA_FONT_SRC: 'https://e.example.com',
    })
    expect(opts.cspExtra?.['connect-src']).toEqual(['https://a.example.com'])
    expect(opts.cspExtra?.['img-src']).toEqual(['https://b.example.com'])
    expect(opts.cspExtra?.['script-src']).toEqual(['https://c.example.com'])
    expect(opts.cspExtra?.['style-src']).toEqual(['https://d.example.com'])
    expect(opts.cspExtra?.['font-src']).toEqual(['https://e.example.com'])
  })
})

describe('response shape (real Response/Headers)', () => {
  it('attaches the full prod header set with env overrides to a real Response', () => {
    const env: NodeJS.ProcessEnv = {
      NODE_ENV: 'production',
      CSP_REPORT_URI: 'https://reports.example.com/csp',
      CSP_EXTRA_CONNECT_SRC: 'https://api.example.com, https://api2.example.com',
      HSTS_PRELOAD: 'true',
      HSTS_MAX_AGE: '63072000',
    }
    const res = new Response('<html></html>', { status: 200 })
    const opts = parseSecurityHeadersEnv(env)
    const headers = buildSecurityHeaders({
      nonce: NONCE,
      isDev: false,
      ...opts,
    })
    for (const [k, v] of Object.entries(headers)) res.headers.set(k, v)

    expect(res.headers.get('Strict-Transport-Security')).toBe(
      'max-age=63072000; includeSubDomains; preload',
    )
    expect(res.headers.get('Cross-Origin-Opener-Policy')).toBe('same-origin')
    expect(res.headers.get('Cross-Origin-Resource-Policy')).toBe('same-origin')
    expect(res.headers.get('Referrer-Policy')).toBe(
      'strict-origin-when-cross-origin',
    )
    expect(res.headers.get('X-Content-Type-Options')).toBe('nosniff')
    expect(res.headers.get('Permissions-Policy')).toContain('payment=()')
    expect(res.headers.get('X-Frame-Options')).toBeNull()

    const csp = res.headers.get('Content-Security-Policy') ?? ''
    expect(csp).toContain(
      "connect-src 'self' https://api.example.com https://api2.example.com",
    )
    expect(csp).toContain(`script-src 'self' 'nonce-${NONCE}'`)
    expect(csp).toContain('report-uri https://reports.example.com/csp')
    expect(res.headers.get('Content-Security-Policy-Report-Only')).toBeNull()
  })

  it('attaches dev defaults (Report-Only, no HSTS) to a real Response', () => {
    const res = new Response('<html></html>', { status: 200 })
    const opts = parseSecurityHeadersEnv({})
    const headers = buildSecurityHeaders({
      nonce: NONCE,
      isDev: true,
      ...opts,
    })
    for (const [k, v] of Object.entries(headers)) res.headers.set(k, v)

    expect(res.headers.get('Content-Security-Policy-Report-Only')).toBeTruthy()
    expect(res.headers.get('Content-Security-Policy')).toBeNull()
    expect(res.headers.get('Strict-Transport-Security')).toBeNull()
    expect(res.headers.get('Cross-Origin-Opener-Policy')).toBe('same-origin')
  })

  it('attaches a CloudFront-managed scenario (HSTS off, COOP off) to a real Response', () => {
    const env: NodeJS.ProcessEnv = {
      NODE_ENV: 'production',
      HSTS_DISABLED: 'true',
      COOP_DISABLED: 'true',
    }
    const res = new Response('<html></html>', { status: 200 })
    const opts = parseSecurityHeadersEnv(env)
    const headers = buildSecurityHeaders({
      nonce: NONCE,
      isDev: false,
      ...opts,
    })
    for (const [k, v] of Object.entries(headers)) res.headers.set(k, v)

    expect(res.headers.get('Strict-Transport-Security')).toBeNull()
    expect(res.headers.get('Cross-Origin-Opener-Policy')).toBeNull()
    expect(res.headers.get('Cross-Origin-Resource-Policy')).toBe('same-origin')
    expect(res.headers.get('Content-Security-Policy')).toBeTruthy()
  })
})
