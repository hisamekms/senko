import {
  GetSecretValueCommand,
  SecretsManagerClient,
} from '@aws-sdk/client-secrets-manager'

export type AuthSecretName = 'AUTH_SECRET' | 'AUTH_OIDC_CLIENT_SECRET'

const TTL_MS = 15 * 60 * 1000

const ARN_REGEX = /^arn:aws:secretsmanager:[a-z0-9-]+:\d{12}:secret:.+$/

interface CacheEntry {
  value: Promise<string>
  fetchedAt: number
}

let _client: SecretsManagerClient | undefined
const cache = new Map<string, CacheEntry>()
const warnedOnce = new Set<AuthSecretName>()

export function isLikelyArn(value: string): boolean {
  return ARN_REGEX.test(value)
}

function getClient(): SecretsManagerClient {
  if (!_client) {
    _client = new SecretsManagerClient({})
  }
  return _client
}

async function fetchFromSecretsManager(arn: string): Promise<string> {
  const client = getClient()
  const response = await client.send(
    new GetSecretValueCommand({ SecretId: arn }),
  )
  if (typeof response.SecretString !== 'string') {
    throw new Error(
      `Secrets Manager response for ${arn} has no SecretString (binary secrets are not supported)`,
    )
  }
  return response.SecretString
}

function getCachedOrFetch(arn: string): Promise<string> {
  const now = Date.now()
  const cached = cache.get(arn)
  if (cached && now - cached.fetchedAt < TTL_MS) {
    return cached.value
  }
  const value = fetchFromSecretsManager(arn)
  cache.set(arn, { value, fetchedAt: now })
  // If the fetch fails, evict so the next call retries.
  value.catch(() => {
    const current = cache.get(arn)
    if (current && current.value === value) {
      cache.delete(arn)
    }
  })
  return value
}

export async function resolveSecret(
  name: AuthSecretName,
): Promise<string | undefined> {
  const arn = process.env[`${name}_ARN`]
  const raw = process.env[name]

  if (arn && arn.length > 0) {
    if (raw && raw.length > 0 && !warnedOnce.has(name)) {
      warnedOnce.add(name)
      console.warn(
        `[secrets/resolve] Both ${name} and ${name}_ARN are set; ${name}_ARN takes precedence and the raw env value is ignored.`,
      )
    }
    return getCachedOrFetch(arn)
  }

  if (raw && raw.length > 0) {
    return raw
  }

  return undefined
}

export function __resetSecretCacheForTests(): void {
  _client = undefined
  cache.clear()
  warnedOnce.clear()
}
