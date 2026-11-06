// Tokens live only in this page's memory, never in localStorage or persisted suites.
const tokens = new Map<string, string>()

export async function qaAuthCacheKey(context: unknown): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(context)))
  return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('')
}

export function getQaAuthToken(key: string) { return tokens.get(key) }
export function saveQaAuthToken(key: string, token: string) {
  // Keep an upper bound when switching among workspaces and credentials.
  if (!tokens.has(key) && tokens.size >= 32) tokens.delete(tokens.keys().next().value!)
  tokens.set(key, token)
}
export function clearQaAuthToken(key: string) { tokens.delete(key) }
