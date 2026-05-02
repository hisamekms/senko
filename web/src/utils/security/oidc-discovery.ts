interface DiscoveryDocument {
  end_session_endpoint?: string
  token_endpoint?: string
}

interface CacheEntry {
  doc: DiscoveryDocument | null
  expiresAt: number
}

const POSITIVE_TTL_MS = 60 * 60 * 1000
const NEGATIVE_TTL_MS = 60 * 1000

let cache: CacheEntry | null = null
let inFlight: Promise<DiscoveryDocument | null> | null = null

async function fetchDiscovery(): Promise<DiscoveryDocument | null> {
  const issuer = process.env.AUTH_OIDC_ISSUER
  if (!issuer) return null

  const url = `${issuer.replace(/\/$/, '')}/.well-known/openid-configuration`
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(5_000) })
    if (!res.ok) return null
    return (await res.json()) as DiscoveryDocument
  } catch {
    return null
  }
}

async function getDiscovery(): Promise<DiscoveryDocument | null> {
  const now = Date.now()
  if (cache && cache.expiresAt > now) return cache.doc
  if (inFlight) return inFlight

  inFlight = fetchDiscovery()
    .then((doc) => {
      cache = {
        doc,
        expiresAt: Date.now() + (doc ? POSITIVE_TTL_MS : NEGATIVE_TTL_MS),
      }
      return doc
    })
    .finally(() => {
      inFlight = null
    })

  return inFlight
}

export async function getEndSessionEndpoint(): Promise<string | null> {
  const doc = await getDiscovery()
  return doc?.end_session_endpoint ?? null
}

export async function getTokenEndpoint(): Promise<string | null> {
  const doc = await getDiscovery()
  return doc?.token_endpoint ?? null
}
