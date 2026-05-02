import { describe, it, expect, afterEach, vi } from 'vitest'

import { assertProductionConfig } from './assert-prod'

const VALID_ARN_AUTH =
  'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/auth-AbCdEf'
const VALID_ARN_OIDC =
  'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/oidc-GhIjKl'

function stubProdHappyEnv(): void {
  vi.stubEnv('NODE_ENV', 'production')
  vi.stubEnv('AUTH_URL', 'https://app.example.com/api/auth')
  vi.stubEnv('AUTH_OIDC_ISSUER', 'https://idp.example.com/realms/senko')
  vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'false')
  vi.stubEnv('AUTH_SECRET', 'raw-auth-secret')
  vi.stubEnv('AUTH_OIDC_CLIENT_SECRET', 'raw-oidc-secret')
}

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
    stubProdHappyEnv()

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('throws in production when AUTH_URL is HTTP', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_URL', 'http://app.example.com/api/auth')

    expect(() => assertProductionConfig()).toThrowError(/AUTH_URL must use https:\/\//)
  })

  it('throws in production when AUTH_OIDC_ISSUER is HTTP', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_OIDC_ISSUER', 'http://idp.example.com/realms/senko')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_OIDC_ISSUER must use https:\/\//,
    )
  })

  it('throws in production when WEB_DEV_AUTH_BYPASS is true (F-4 regression)', () => {
    stubProdHappyEnv()
    vi.stubEnv('WEB_DEV_AUTH_BYPASS', 'true')

    expect(() => assertProductionConfig()).toThrowError(/WEB_DEV_AUTH_BYPASS=true/)
  })

  it('aggregates every failure into a single thrown error', () => {
    stubProdHappyEnv()
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
    stubProdHappyEnv()
    vi.stubEnv('AUTH_URL', '')

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('does not throw in production when AUTH_OIDC_ISSUER is unset (presence is not asserted)', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_OIDC_ISSUER', '')

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('accepts ARN form for both secrets in production', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_SECRET', '')
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET', '')
    vi.stubEnv('AUTH_SECRET_ARN', VALID_ARN_AUTH)
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET_ARN', VALID_ARN_OIDC)

    expect(() => assertProductionConfig()).not.toThrow()
  })

  it('throws in production when neither AUTH_SECRET nor AUTH_SECRET_ARN is set', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_SECRET', '')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_SECRET is required in production \(set AUTH_SECRET or AUTH_SECRET_ARN\)/,
    )
  })

  it('throws in production when neither AUTH_OIDC_CLIENT_SECRET nor AUTH_OIDC_CLIENT_SECRET_ARN is set', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET', '')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_OIDC_CLIENT_SECRET is required in production \(set AUTH_OIDC_CLIENT_SECRET or AUTH_OIDC_CLIENT_SECRET_ARN\)/,
    )
  })

  it('throws in production when AUTH_SECRET_ARN is malformed', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_SECRET_ARN', 'not-an-arn')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_SECRET_ARN does not look like a Secrets Manager ARN/,
    )
  })

  it('throws in production when AUTH_OIDC_CLIENT_SECRET_ARN is malformed', () => {
    stubProdHappyEnv()
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET_ARN', 'arn:aws:s3:::my-bucket')

    expect(() => assertProductionConfig()).toThrowError(
      /AUTH_OIDC_CLIENT_SECRET_ARN does not look like a Secrets Manager ARN/,
    )
  })
})
