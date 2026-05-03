export interface SignInGateConfig {
  requiredScope: string | null
  requiredGroups: string[] | null
  groupsClaim: string | null
}

export type GateRejectReason =
  | 'groups_claim_misconfigured'
  | 'token_missing'
  | 'token_decode_failed'
  | 'scope_claim_missing'
  | 'scope_mismatch'
  | 'groups_claim_missing'
  | 'groups_mismatch'

export type GateResult =
  | { allowed: true }
  | {
      allowed: false
      reason: GateRejectReason
      details?: { scopes?: string[]; groups?: string[] }
    }

function readNonEmpty(value: string | undefined): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

export function loadSignInGateConfig(
  env: NodeJS.ProcessEnv,
): SignInGateConfig {
  const requiredScope = readNonEmpty(env.SENKO_AUTH_REQUIRED_SCOPE)
  const groupsClaim = readNonEmpty(env.SENKO_AUTH_GROUPS_CLAIM)

  const groupsRaw = readNonEmpty(env.SENKO_AUTH_REQUIRED_GROUPS)
  let requiredGroups: string[] | null = null
  if (groupsRaw !== null) {
    const parsed = groupsRaw
      .split(',')
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
    requiredGroups = parsed.length > 0 ? parsed : null
  }

  return { requiredScope, requiredGroups, groupsClaim }
}

export function decodeJwtPayload(
  token: string,
): Record<string, unknown> | null {
  const parts = token.split('.')
  if (parts.length !== 3) return null
  try {
    const json = Buffer.from(parts[1], 'base64url').toString('utf-8')
    if (json.length === 0) return null
    const parsed: unknown = JSON.parse(json)
    if (
      typeof parsed === 'object' &&
      parsed !== null &&
      !Array.isArray(parsed)
    ) {
      return parsed as Record<string, unknown>
    }
    return null
  } catch {
    return null
  }
}

export function parseGroupsClaim(value: unknown): string[] | null {
  if (Array.isArray(value)) {
    const out = value.filter((v): v is string => typeof v === 'string')
    return out.length > 0 ? out : null
  }
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (trimmed.length === 0) return null

  if (trimmed.startsWith('[')) {
    try {
      const parsed: unknown = JSON.parse(trimmed)
      if (Array.isArray(parsed)) {
        const out = parsed.filter((v): v is string => typeof v === 'string')
        return out.length > 0 ? out : null
      }
    } catch {
      // fall through to delimiter parsing
    }
  }

  const delimiter = trimmed.includes(',') ? ',' : /\s+/
  const out = trimmed
    .split(delimiter)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
  return out.length > 0 ? out : null
}

export function evaluateSignInGate(
  config: SignInGateConfig,
  accessToken: string | undefined,
): GateResult {
  if (config.requiredScope === null && config.requiredGroups === null) {
    return { allowed: true }
  }

  if (config.requiredGroups !== null && config.groupsClaim === null) {
    return { allowed: false, reason: 'groups_claim_misconfigured' }
  }

  if (!accessToken) {
    return { allowed: false, reason: 'token_missing' }
  }

  const payload = decodeJwtPayload(accessToken)
  if (payload === null) {
    return { allowed: false, reason: 'token_decode_failed' }
  }

  if (config.requiredScope !== null) {
    const raw = payload['scope']
    if (typeof raw !== 'string') {
      return { allowed: false, reason: 'scope_claim_missing' }
    }
    const scopes = raw.split(/\s+/).filter((s) => s.length > 0)
    if (!scopes.includes(config.requiredScope)) {
      return {
        allowed: false,
        reason: 'scope_mismatch',
        details: { scopes },
      }
    }
  }

  if (config.requiredGroups !== null) {
    const claimKey = config.groupsClaim as string
    const groups = parseGroupsClaim(payload[claimKey])
    if (groups === null) {
      return { allowed: false, reason: 'groups_claim_missing' }
    }
    const matched = config.requiredGroups.some((g) => groups.includes(g))
    if (!matched) {
      return {
        allowed: false,
        reason: 'groups_mismatch',
        details: { groups },
      }
    }
  }

  return { allowed: true }
}

export function maskEmail(
  email: string | null | undefined,
): string | null {
  if (email === null || email === undefined) return null
  const at = email.indexOf('@')
  if (at <= 0) return email
  const local = email.slice(0, at)
  const domain = email.slice(at + 1)
  return `${local[0]}***@${domain}`
}
