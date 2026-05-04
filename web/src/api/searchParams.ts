// Tiny coercion helpers shared by route validateSearch implementations.
// TanStack Router hands us `Record<string, unknown>` from the URL, which we
// narrow into the typed Search shape used by each route.

export function asStringArray(v: unknown): string[] | undefined {
  if (Array.isArray(v)) {
    const out = v.filter((x): x is string => typeof x === 'string')
    return out.length > 0 ? out : undefined
  }
  if (typeof v === 'string') {
    return v.length > 0 ? [v] : undefined
  }
  return undefined
}

export function asString(v: unknown): string | undefined {
  return typeof v === 'string' && v.length > 0 ? v : undefined
}

export function asBoolean(v: unknown): boolean | undefined {
  if (typeof v === 'boolean') return v
  if (typeof v === 'string') {
    if (v === 'true') return true
    if (v === 'false') return false
  }
  return undefined
}

export function asNumber(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v
  if (typeof v === 'string' && v.length > 0) {
    const n = Number(v)
    if (Number.isFinite(n) && Number.isInteger(n) && n > 0) return n
  }
  return undefined
}

export function asOrder(v: unknown): 'asc' | 'desc' | undefined {
  if (v === 'asc' || v === 'desc') return v
  return undefined
}
