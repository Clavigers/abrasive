import { useEffect, useState } from 'react'
import type { Session } from '@supabase/supabase-js'
import TopBar from '../TopBar'
import {
  createToken,
  deleteToken,
  listTokens,
  type CreatedToken,
  type PublicToken,
} from '../lib/tokens'

type Props = { session: Session }

export default function Tokens({ session }: Props) {
  const [tokens, setTokens] = useState<PublicToken[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [name, setName] = useState('')
  const [creating, setCreating] = useState(false)
  const [justCreated, setJustCreated] = useState<CreatedToken | null>(null)

  const refresh = async () => {
    try {
      setTokens(await listTokens())
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  useEffect(() => { refresh() }, [])

  const onCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    setCreating(true)
    setError(null)
    try {
      const result = await createToken(name.trim())
      setJustCreated(result)
      setName('')
      setShowForm(false)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreating(false)
    }
  }

  const onDelete = async (id: string) => {
    if (!confirm('Delete this token? CLIs using it will stop working.')) return
    try {
      await deleteToken(id)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="app">
      <TopBar session={session} />
      <main className="page">
        <header className="page-header">
          <h1>API Tokens</h1>
          {!showForm && !justCreated && (
            <button className="primary-btn" onClick={() => setShowForm(true)}>New Token</button>
          )}
        </header>

        <p className="muted">
          Use these to run abrasive commands from the CLI. Tokens are stored hashed —
          you'll only see the full value once, right after creation.
        </p>

        {error && <div className="error">{error}</div>}

        {justCreated && (
          <div className="created-banner">
            <div className="created-row">
              <strong>{justCreated.row.name}</strong>
              <button className="link-btn" onClick={() => setJustCreated(null)}>Dismiss</button>
            </div>
            <p className="muted">Copy this now. You won't see it again.</p>
            <code className="token-display">{justCreated.plaintext}</code>
            <button
              className="primary-btn"
              onClick={() => navigator.clipboard.writeText(justCreated.plaintext)}
            >Copy</button>
          </div>
        )}

        {showForm && (
          <form className="token-form" onSubmit={onCreate}>
            <input
              type="text"
              placeholder="Token name (e.g. work-laptop)"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              disabled={creating}
            />
            <button type="submit" className="primary-btn" disabled={creating || !name.trim()}>
              {creating ? 'Creating…' : 'Create'}
            </button>
            <button type="button" className="link-btn" onClick={() => setShowForm(false)} disabled={creating}>
              Cancel
            </button>
          </form>
        )}

        {tokens === null ? (
          <p className="muted">Loading…</p>
        ) : tokens.length === 0 ? (
          <p className="muted">No tokens yet.</p>
        ) : (
          <table className="token-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Prefix</th>
                <th>Created</th>
                <th>Last used</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {tokens.map((t) => (
                <tr key={t.id}>
                  <td>{t.name}</td>
                  <td><code>abrasive_{t.prefix}…</code></td>
                  <td>{new Date(t.created_at).toLocaleDateString()}</td>
                  <td>{t.last_used_at ? new Date(t.last_used_at).toLocaleDateString() : '—'}</td>
                  <td><button className="link-btn danger" onClick={() => onDelete(t.id)}>Revoke</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </main>
    </div>
  )
}
