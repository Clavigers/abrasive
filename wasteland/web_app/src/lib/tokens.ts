import { supabase } from './supabase'

export type Scope = 'read' | 'read-write'

export type PublicToken = {
  id: string
  user_id: string
  name: string
  prefix: string
  scope: Scope
  created_at: string
  expires_at: string | null
  last_used_at: string | null
}

const TOKEN_PREFIX = 'abrasive_'
const TOKEN_BYTES = 32

const base64url = (bytes: Uint8Array) =>
  btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')

const sha256Hex = async (input: string) => {
  const data = new TextEncoder().encode(input)
  const hash = await crypto.subtle.digest('SHA-256', data)
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

export type CreatedToken = { plaintext: string; row: PublicToken }

export async function createToken(
  name: string,
  scope: Scope,
  expiresAt: Date | null,
): Promise<CreatedToken> {
  const { data: sessionData } = await supabase.auth.getSession()
  const userId = sessionData.session?.user.id
  if (!userId) throw new Error('not signed in')

  const randomBytes = crypto.getRandomValues(new Uint8Array(TOKEN_BYTES))
  const body = base64url(randomBytes)
  const plaintext = TOKEN_PREFIX + body
  const token_hash = await sha256Hex(plaintext)
  const prefix = body.slice(0, 6)

  const { data, error } = await supabase
    .from('api_tokens')
    .insert({
      user_id: userId,
      name,
      token_hash,
      prefix,
      scope,
      expires_at: expiresAt?.toISOString() ?? null,
    })
    .select('id, user_id, name, prefix, scope, created_at, expires_at, last_used_at')
    .single()

  if (error) throw error
  return { plaintext, row: data as PublicToken }
}

export async function regenerateToken(old: PublicToken): Promise<CreatedToken> {
  let newExpiresAt: Date | null = null
  if (old.expires_at) {
    const originalDuration =
      new Date(old.expires_at).getTime() - new Date(old.created_at).getTime()
    newExpiresAt = new Date(Date.now() + originalDuration)
  }
  await deleteToken(old.id)
  return createToken(old.name, old.scope, newExpiresAt)
}

export async function listTokens(): Promise<PublicToken[]> {
  const { data, error } = await supabase
    .from('api_tokens_public')
    .select('*')
    .order('created_at', { ascending: false })
  if (error) throw error
  return (data ?? []) as PublicToken[]
}

export async function deleteToken(id: string): Promise<void> {
  const { error } = await supabase.from('api_tokens').delete().eq('id', id)
  if (error) throw error
}
