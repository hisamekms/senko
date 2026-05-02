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

beforeEach(() => {
  clientCtorMock.mockReset()
  sendMock.mockReset()
  vi.unstubAllEnvs()
  vi.resetModules()
})

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('auth modules — module init does not fetch secrets', () => {
  it('importing the resolver, auth/index, and auth/refresh with all secret envs unset does not construct SecretsManagerClient', async () => {
    await import('#/utils/secrets/resolve')
    await import('#/utils/auth')
    await import('#/utils/auth/refresh')

    expect(clientCtorMock).not.toHaveBeenCalled()
    expect(sendMock).not.toHaveBeenCalled()
  })

  it('importing the auth modules with AUTH_SECRET_ARN set still does not construct the client (resolution must be lazy)', async () => {
    vi.stubEnv(
      'AUTH_SECRET_ARN',
      'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/auth-AbCdEf',
    )
    vi.stubEnv(
      'AUTH_OIDC_CLIENT_SECRET_ARN',
      'arn:aws:secretsmanager:ap-northeast-1:111122223333:secret:senko/oidc-GhIjKl',
    )

    await import('#/utils/secrets/resolve')
    await import('#/utils/auth')
    await import('#/utils/auth/refresh')

    expect(clientCtorMock).not.toHaveBeenCalled()
    expect(sendMock).not.toHaveBeenCalled()
  })
})
