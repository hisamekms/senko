import { describe, it, expect, afterEach, vi } from 'vitest'

import { assertProductionConfig } from './assert-prod'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('assertProductionConfig', () => {
  it('returns silently in dev even with HTTP URLs and bypass=true', () => {
    vi.stubEnv('NODE_ENV', 'development')
    vi.stubEnv('AUTH_URL', 'http://localhost:3000/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'http://localhost:8081/realms/senko')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'true')

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('returns silently in production when both URLs are https and bypass is off', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'https://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'https://idp.example.com/realms/senko')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'false')

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('throws in production when AUTH_URL is HTTP', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'http://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'https://idp.example.com/realms/senko')

    expect(() => assertProductionConfig()).toThrowError(/AUTH_URL must use https:\/\//)
  })

  it('throws in production when AUTH_OIDC_ISSUER is HTTP', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'https://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'http://idp.example.com/realms/senko')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_OIDC_ISSUER must use https:\/\//,
    )
  })

  it('throws in production when WEB_DEV_AUTH_BYPASS is true (F-4 regression)', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'https://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'https://idp.example.com/realms/senko')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'true')

    expect(() => assertProductionConfig()).toThrowError(/WEB_DEV_AUTH_BYPASS=true/)
  })

  it('aggregates every failure into a single thrown error', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'http://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'http://idp.example.com/realms/senko')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'true')

    let captured: unknown
    try {
      assertProductionConfig()
    } catch (e) {
      captured = e
    }

    expect(captured).toBeInstanceOf(Error)
    const msg = (captured as Error).message
    expect(msg).toContain('WEB_DEV_AUTH_BYPASS=true')
    expect(msg).toContain('AUTH_URL must use https://')
    expect(msg).toContain('AUTH_OIDC_ISSUER must use https://')
  })

  it('does not throw in production when AUTH_URL is unset (presence is not asserted)', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', '')
    vi.stubEnv('AUTH_OIDC_ISSUER', 'https://idp.example.com/realms/senko')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'false')

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('does not throw in production when AUTH_OIDC_ISSUER is unset (presence is not asserted)', () => {
    vi.stubEnv('NODE_ENV', 'production')
    vi.stubEnv('AUTH_URL', 'https://app.example.com/api/auth')
    vi.stubEnv('AUTH_OIDC_ISSUER', '')
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'false')

    expect(() => assertProductionConfig()).not.toThrow()
  })
})
