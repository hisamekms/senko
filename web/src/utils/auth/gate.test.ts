import { describe, expect, it } from 'vitest'

import {
  decodeJwtPayload,
  evaluateSignInGate,
  loadSignInGateConfig,
  maskEmail,
  parseGroupsClaim,
  type SignInGateConfig,
} from './gate'

function b64url(input: string): string {
  return Buffer.from(input, 'utf-8').toString('base64url')
}

function makeJwt(payload: Record<string, unknown>): string {
  const header = b64url(JSON.stringify({ alg: 'none', typ: 'JWT' }))
  const body = b64url(JSON.stringify(payload))
  return `${header}.${body}.sig`
}

function gateConfig(
  overrides: Partial<SignInGateConfig> = {},
): SignInGateConfig {
  return {
    requiredScope: null,
    requiredGroups: null,
    groupsClaim: null,
    ...overrides,
  }
}

describe('evaluateSignInGate', () => {
  it('allows when both required envs are unset (backward compatible)', () => {
    const result = evaluateSignInGate(gateConfig(), makeJwt({ scope: 'a' }))
    expect(result).toEqual({ allowed: true })
  })

  it('allows when scope is set and matches', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const token = makeJwt({ scope: 'openid profile senko:access email' })
    expect(evaluateSignInGate(config, token)).toEqual({ allowed: true })
  })

  it('rejects with scope_mismatch when scope is set but missing in token', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const token = makeJwt({ scope: 'openid profile email' })
    const result = evaluateSignInGate(config, token)
    expect(result).toEqual({
      allowed: false,
      reason: 'scope_mismatch',
      details: { scopes: ['openid', 'profile', 'email'] },
    })
  })

  it('rejects with scope_claim_missing when scope claim is absent', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const token = makeJwt({ sub: 'u1' })
    const result = evaluateSignInGate(config, token)
    expect(result).toEqual({
      allowed: false,
      reason: 'scope_claim_missing',
    })
  })

  it('rejects with scope_claim_missing when scope claim is non-string', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const token = makeJwt({ scope: ['senko:access'] })
    const result = evaluateSignInGate(config, token)
    expect(result).toEqual({
      allowed: false,
      reason: 'scope_claim_missing',
    })
  })

  it('rejects with scope_mismatch when scope claim is empty / whitespace-only', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const token = makeJwt({ scope: '   ' })
    const result = evaluateSignInGate(config, token)
    expect(result).toEqual({
      allowed: false,
      reason: 'scope_mismatch',
      details: { scopes: [] },
    })
  })

  it('allows when groups (with claim) intersect by ANY', () => {
    const config = gateConfig({
      requiredGroups: ['senko', 'senko-admin'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({ senko_groups: ['senko', 'other'] })
    expect(evaluateSignInGate(config, token)).toEqual({ allowed: true })
  })

  it('rejects with groups_mismatch when no group overlaps', () => {
    const config = gateConfig({
      requiredGroups: ['senko'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({ senko_groups: ['aviary', 'haimate-app-prod'] })
    const result = evaluateSignInGate(config, token)
    expect(result).toEqual({
      allowed: false,
      reason: 'groups_mismatch',
      details: { groups: ['aviary', 'haimate-app-prod'] },
    })
  })

  it('rejects with groups_claim_missing when groups claim is absent', () => {
    const config = gateConfig({
      requiredGroups: ['senko'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({ sub: 'u1' })
    expect(evaluateSignInGate(config, token)).toEqual({
      allowed: false,
      reason: 'groups_claim_missing',
    })
  })

  it('allows when both scope and groups are set and both pass (AND)', () => {
    const config = gateConfig({
      requiredScope: 'senko:access',
      requiredGroups: ['senko'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({
      scope: 'openid senko:access',
      senko_groups: ['senko'],
    })
    expect(evaluateSignInGate(config, token)).toEqual({ allowed: true })
  })

  it('rejects with groups_mismatch when AND gate has scope ok but groups fail', () => {
    const config = gateConfig({
      requiredScope: 'senko:access',
      requiredGroups: ['senko'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({
      scope: 'openid senko:access',
      senko_groups: ['aviary'],
    })
    const result = evaluateSignInGate(config, token)
    expect(result.allowed).toBe(false)
    if (result.allowed) throw new Error('unreachable')
    expect(result.reason).toBe('groups_mismatch')
  })

  it('rejects with scope_mismatch when AND gate has groups ok but scope fails', () => {
    const config = gateConfig({
      requiredScope: 'senko:access',
      requiredGroups: ['senko'],
      groupsClaim: 'senko_groups',
    })
    const token = makeJwt({
      scope: 'openid profile',
      senko_groups: ['senko'],
    })
    const result = evaluateSignInGate(config, token)
    expect(result.allowed).toBe(false)
    if (result.allowed) throw new Error('unreachable')
    expect(result.reason).toBe('scope_mismatch')
  })

  it('rejects with token_missing when accessToken is undefined under active gate', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    expect(evaluateSignInGate(config, undefined)).toEqual({
      allowed: false,
      reason: 'token_missing',
    })
  })

  it('rejects with token_missing when accessToken is empty string', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    expect(evaluateSignInGate(config, '')).toEqual({
      allowed: false,
      reason: 'token_missing',
    })
  })

  it('rejects with token_decode_failed when token has wrong segment count', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    expect(evaluateSignInGate(config, 'not-a-jwt')).toEqual({
      allowed: false,
      reason: 'token_decode_failed',
    })
  })

  it('rejects with token_decode_failed when payload segment is not valid JSON', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const broken = `e30.${b64url('<<not json>>')}.sig`
    expect(evaluateSignInGate(config, broken)).toEqual({
      allowed: false,
      reason: 'token_decode_failed',
    })
  })

  it('rejects with token_decode_failed when payload is a JSON array', () => {
    const config = gateConfig({ requiredScope: 'senko:access' })
    const arr = `e30.${b64url('["senko:access"]')}.sig`
    expect(evaluateSignInGate(config, arr)).toEqual({
      allowed: false,
      reason: 'token_decode_failed',
    })
  })

  it('rejects with groups_claim_misconfigured when REQUIRED_GROUPS is set but GROUPS_CLAIM is unset (regardless of token)', () => {
    const config = gateConfig({
      requiredGroups: ['senko'],
      groupsClaim: null,
    })
    const token = makeJwt({ senko_groups: ['senko'], scope: 'whatever' })
    expect(evaluateSignInGate(config, token)).toEqual({
      allowed: false,
      reason: 'groups_claim_misconfigured',
    })
    expect(evaluateSignInGate(config, undefined)).toEqual({
      allowed: false,
      reason: 'groups_claim_misconfigured',
    })
  })
})

describe('parseGroupsClaim', () => {
  it('returns string entries from JS arrays', () => {
    expect(parseGroupsClaim(['a', 'b', 1, null, 'c'])).toEqual(['a', 'b', 'c'])
  })

  it('returns null for an array with no string entries', () => {
    expect(parseGroupsClaim([1, null, {}])).toBeNull()
  })

  it('parses a JSON-array string', () => {
    expect(parseGroupsClaim('["senko","admin"]')).toEqual(['senko', 'admin'])
  })

  it('falls back to delimiter parsing when JSON-array parse fails', () => {
    // Starts with `[` but is not valid JSON → fall through to delimiter split.
    expect(parseGroupsClaim('[broken,senko')).toEqual(['[broken', 'senko'])
  })

  it('parses a comma-separated string with whitespace', () => {
    expect(parseGroupsClaim('senko, senko-admin , aviary')).toEqual([
      'senko',
      'senko-admin',
      'aviary',
    ])
  })

  it('parses a space-separated string', () => {
    expect(parseGroupsClaim('senko senko-admin aviary')).toEqual([
      'senko',
      'senko-admin',
      'aviary',
    ])
  })

  it('parses a tab-separated / mixed-whitespace string', () => {
    expect(parseGroupsClaim('a\tb  c')).toEqual(['a', 'b', 'c'])
  })

  it('returns null for an empty / whitespace-only string', () => {
    expect(parseGroupsClaim('')).toBeNull()
    expect(parseGroupsClaim('   ')).toBeNull()
  })

  it('returns null for null / undefined / non-string-non-array', () => {
    expect(parseGroupsClaim(null)).toBeNull()
    expect(parseGroupsClaim(undefined)).toBeNull()
    expect(parseGroupsClaim({ a: 1 })).toBeNull()
    expect(parseGroupsClaim(42)).toBeNull()
  })
})

describe('loadSignInGateConfig', () => {
  it('returns all-null when no env vars are set', () => {
    expect(loadSignInGateConfig({})).toEqual({
      requiredScope: null,
      requiredGroups: null,
      groupsClaim: null,
    })
  })

  it('trims whitespace from values', () => {
    const config = loadSignInGateConfig({
      SENKO_AUTH_REQUIRED_SCOPE: '  senko:access  ',
      SENKO_AUTH_GROUPS_CLAIM: '  senko_groups  ',
      SENKO_AUTH_REQUIRED_GROUPS: '  senko ,  senko-admin  ',
    })
    expect(config).toEqual({
      requiredScope: 'senko:access',
      groupsClaim: 'senko_groups',
      requiredGroups: ['senko', 'senko-admin'],
    })
  })

  it('treats whitespace-only env values as unset', () => {
    expect(
      loadSignInGateConfig({
        SENKO_AUTH_REQUIRED_SCOPE: '   ',
        SENKO_AUTH_GROUPS_CLAIM: '\t',
        SENKO_AUTH_REQUIRED_GROUPS: ' ',
      }),
    ).toEqual({
      requiredScope: null,
      requiredGroups: null,
      groupsClaim: null,
    })
  })

  it("parses ',' (only commas/whitespace) to null, not empty allow-list", () => {
    expect(
      loadSignInGateConfig({
        SENKO_AUTH_REQUIRED_GROUPS: ' , , ',
      }).requiredGroups,
    ).toBeNull()
  })

  it('treats single-group env as a 1-element array', () => {
    expect(
      loadSignInGateConfig({ SENKO_AUTH_REQUIRED_GROUPS: 'senko' })
        .requiredGroups,
    ).toEqual(['senko'])
  })
})

describe('decodeJwtPayload', () => {
  it('decodes a valid 3-segment JWT payload', () => {
    const token = makeJwt({ sub: 'u1', scope: 'a' })
    expect(decodeJwtPayload(token)).toEqual({ sub: 'u1', scope: 'a' })
  })

  it('returns null when the token does not have 3 segments', () => {
    expect(decodeJwtPayload('only.two')).toBeNull()
    expect(decodeJwtPayload('a.b.c.d')).toBeNull()
    expect(decodeJwtPayload('opaque-token')).toBeNull()
  })

  it('returns null when the payload segment is not valid JSON', () => {
    expect(decodeJwtPayload(`e30.${b64url('not-json')}.sig`)).toBeNull()
  })

  it('returns null when the payload is not an object (array, primitive)', () => {
    expect(decodeJwtPayload(`e30.${b64url('[1,2,3]')}.sig`)).toBeNull()
    expect(decodeJwtPayload(`e30.${b64url('"plain-string"')}.sig`)).toBeNull()
    expect(decodeJwtPayload(`e30.${b64url('null')}.sig`)).toBeNull()
  })
})

describe('maskEmail', () => {
  it('masks the local part except the first character', () => {
    expect(maskEmail('alice@example.com')).toBe('a***@example.com')
  })

  it('returns null for null / undefined', () => {
    expect(maskEmail(null)).toBeNull()
    expect(maskEmail(undefined)).toBeNull()
  })

  it("returns the input unchanged when there is no '@' or it's at the start", () => {
    expect(maskEmail('no-at-sign')).toBe('no-at-sign')
    expect(maskEmail('@nolocal.com')).toBe('@nolocal.com')
  })
})
