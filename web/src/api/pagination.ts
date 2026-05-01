export interface Page<T> {
  items: T[]
  next_cursor?: string | null
}

export async function* paginate<T>(
  fetchPage: (cursor: string | null) => Promise<Page<T>>,
): AsyncGenerator<T[], void, unknown> {
  let cursor: string | null = null
  while (true) {
    const page = await fetchPage(cursor)
    yield page.items
    cursor = page.next_cursor ?? null
    if (cursor === null) break
  }
}

export async function collectAll<T>(
  fetchPage: (cursor: string | null) => Promise<Page<T>>,
): Promise<T[]> {
  const items: T[] = []
  for await (const batch of paginate(fetchPage)) {
    items.push(...batch)
  }
  return items
}
