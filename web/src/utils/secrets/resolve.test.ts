import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const sendMock = vi.fn()
const clientCtorMock = vi.fn()

vi.mock('@aws-sdk/client-secrets-manager', () => {
  class SecretsManagerClient {
    constructor(...args: unknown[]) {
      clientCtorMock(...args)
    }
    send = sendMock
  }
  class GetSecretValueCommand {
    input: { SecretId?: string }
    constructor(input: { SecretId?: string }) {
      this.input = input
    }
  }
  return { SecretsManagerClient, GetSecretValueCommand }
})

import {
  __resetSecretCacheForTests,
  isLikelyArn,
  resolveSecret,
} from './resolve'

const ARN_AUTH = 'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/auth-AbCdEf'
const ARN_OIDC = 'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/oidc-GhIjKl'

beforeEach(() => {
  __resetSecretCacheForTests()
  sendMock.mockReset()
  clientCtorMock.mockReset()
})

afterEach(() => {
  vi.unstubAllEnvs()
  vi.useRealTimers()
})

describe('resolveSecret', () => {
  it('lazy: importing the module without ARN env does not construct the SDK client', () => {
    expect(clientCtorMock).not.toHaveBeenCalled()
  })

  it('returns the raw env value when only AUTH_SECRET is set, without calling the SDK', async () => {
    vi.stubEnv('AUTH_SECRET', 'raw-secret-value')

    const result = await resolveSecret('AUTH_SECRET')

    expect(result).toBe('raw-secret-value')
    expect(clientCtorMock).not.toHaveBeenCalled()
    expect(sendMock).not.toHaveBeenCalled()
  })

  it('fetches from Secrets Manager when AUTH_SECRET_ARN is set', async () => {
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValueOnce({ SecretString: 'fetched-value' })

    const result = await resolveSecret('AUTH_SECRET')

    expect(result).toBe('fetched-value')
    expect(clientCtorMock).toHaveBeenCalledTimes(1)
    expect(sendMock).toHaveBeenCalledTimes(1)
    const command = sendMock.mock.calls[0][0] as { input: { SecretId: string } }
    expect(command.input.SecretId).toBe(ARN_AUTH)
  })

  it('dedupes concurrent calls into a single SDK request', async () => {
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValueOnce({ SecretString: 'fetched-value' })

    const [a, b] = await Promise.all([
      resolveSecret('AUTH_SECRET'),
      resolveSecret('AUTH_SECRET'),
    ])

    expect(a).toBe('fetched-value')
    expect(b).toBe('fetched-value')
    expect(sendMock).toHaveBeenCalledTimes(1)
  })

  it('caches sequential calls within the 15-minute TTL', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T00:00:00Z'))
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValue({ SecretString: 'fetched-value' })

    await resolveSecret('AUTH_SECRET')
    vi.advanceTimersByTime(14 * 60 * 1000)
    await resolveSecret('AUTH_SECRET')

    expect(sendMock).toHaveBeenCalledTimes(1)
  })

  it('refetches after the TTL expires', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T00:00:00Z'))
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValue({ SecretString: 'fetched-value' })

    await resolveSecret('AUTH_SECRET')
    vi.advanceTimersByTime(15 * 60 * 1000 + 1)
    await resolveSecret('AUTH_SECRET')

    expect(sendMock).toHaveBeenCalledTimes(2)
  })

  it('prefers ARN over raw env and warns exactly once per secret name', async () => {
    vi.stubEnv('AUTH_SECRET', 'raw-ignored')
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValue({ SecretString: 'arn-value' })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const a = await resolveSecret('AUTH_SECRET')
    const b = await resolveSecret('AUTH_SECRET')

    expect(a).toBe('arn-value')
    expect(b).toBe('arn-value')
    expect(warnSpy).toHaveBeenCalledTimes(1)
    expect(warnSpy.mock.calls[0][0]).toContain('AUTH_SECRET')
    expect(warnSpy.mock.calls[0][0]).toContain('AUTH_SECRET_ARN')

    warnSpy.mockRestore()
  })

  it('warns separately for AUTH_OIDC_CLIENT_SECRET', async () => {
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET', 'raw-ignored')
    vi.stubEnv('AUTH_OIDC_CLIENT_SECRET_ARN', ARN_OIDC)
    sendMock.mockResolvedValue({ SecretString: 'oidc-arn-value' })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const a = await resolveSecret('AUTH_OIDC_CLIENT_SECRET')

    expect(a).toBe('oidc-arn-value')
    expect(warnSpy).toHaveBeenCalledTimes(1)
    expect(warnSpy.mock.calls[0][0]).toContain('AUTH_OIDC_CLIENT_SECRET')

    warnSpy.mockRestore()
  })

  it('returns undefined when neither raw nor ARN env is set', async () => {
    const result = await resolveSecret('AUTH_SECRET')

    expect(result).toBeUndefined()
    expect(clientCtorMock).not.toHaveBeenCalled()
    expect(sendMock).not.toHaveBeenCalled()
  })

  it('throws when the SDK response has no SecretString (binary secrets are unsupported)', async () => {
    vi.stubEnv('AUTH_SECRET_ARN', ARN_AUTH)
    sendMock.mockResolvedValueOnce({ SecretBinary: new Uint8Array([1, 2, 3]) })

    await expect(resolveSecret('AUTH_SECRET')).rejects.toThrow(
      /no SecretString/,
    )
  })
})

describe('isLikelyArn', () => {
  it('accepts a well-formed Secrets Manager ARN', () => {
    expect(isLikelyArn(ARN_AUTH)).toBe(true)
  })

  it('rejects strings that do not look like Secrets Manager ARNs', () => {
    expect(isLikelyArn('not-an-arn')).toBe(false)
    expect(isLikelyArn('arn:aws:s3:::my-bucket')).toBe(false)
    expect(isLikelyArn('')).toBe(false)
    expect(isLikelyArn('arn:aws:secretsmanager:us-east-1:abc:secret:foo')).toBe(false)
  })
})
