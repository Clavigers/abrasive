import { useEffect, useState } from 'react'
import type { Session } from '@supabase/supabase-js'
import TopBar from '../TopBar'
import Footer from '../Footer'
import CodeBlock from '../components/CodeBlock'
import {
  createToken,
  deleteToken,
  listTokens,
  regenerateToken,
  type CreatedToken,
  type PublicToken,
  type Scope,
} from '../lib/tokens'

type Props = { session: Session }

const EXPIRATION_OPTIONS: Array<{ label: string; days: number | null }> = [
  { label: '30 days', days: 30 },
  { label: '90 days', days: 90 },
  { label: '1 year', days: 365 },
  { label: 'Never', days: null },
]

function formatDuration(ms: number): string {
  const abs = Math.abs(ms)
  const mins = abs / 60_000
  const hours = mins / 60
  const days = hours / 24
  const months = days / 30.4
  const years = days / 365

  if (mins < 1) return 'less than a minute'
  const round = (n: number, unit: string) =>
    `${Math.round(n)} ${unit}${Math.round(n) === 1 ? '' : 's'}`
  if (mins < 60) return round(mins, 'minute')
  if (hours < 24) return round(hours, 'hour')
  if (days < 30) return round(days, 'day')
  if (months < 12) return round(months, 'month')
  return round(years, 'year')
}

function relativeTimePast(when: string): string {
  return formatDuration(Date.now() - new Date(when).getTime())
}

function expirationText(expires_at: string | null): string {
  if (!expires_at) return 'Never expires'
  const diff = new Date(expires_at).getTime() - Date.now()
  if (diff < 0) return `Expired ${formatDuration(-diff)} ago`
  return `Expires in ${formatDuration(diff)}`
}

function daysFromNow(days: number): Date {
  return new Date(Date.now() + days * 24 * 60 * 60 * 1000)
}

export default function Tokens({ session }: Props) {
  const [tokens, setTokens] = useState<PublicToken[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [name, setName] = useState('')
  const [scope, setScope] = useState<Scope>('read-write')
  const [expirationDays, setExpirationDays] = useState<number | null>(365)
  const [busy, setBusy] = useState(false)
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
    setBusy(true)
    setError(null)
    try {
      const expiresAt = expirationDays === null ? null : daysFromNow(expirationDays)
      const result = await createToken(name.trim(), scope, expiresAt)
      setJustCreated(result)
      setName('')
      setShowForm(false)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const onDelete = async (t: PublicToken) => {
    if (!confirm(`Revoke "${t.name}"? CLIs using it will stop working.`)) return
    setError(null)
    try {
      await deleteToken(t.id)
      if (justCreated?.row.id === t.id) setJustCreated(null)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const onRegenerate = async (t: PublicToken) => {
    if (!confirm(`Regenerate "${t.name}"? The old key stops working immediately.`)) return
    setBusy(true)
    setError(null)
    try {
      const result = await regenerateToken(t)
      setJustCreated(result)
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="app">
      <TopBar session={session} />
      <main className="page">
        <header className="page-header">
          <h1>API Tokens</h1>
          {!showForm && (
            <button className="primary-btn" onClick={() => setShowForm(true)} disabled={busy}>
              New Token
            </button>
          )}
        </header>

        <div className="token-description">
          <p>
            You can use the API tokens generated on this page to run abrasive CLI
            commands that authenticate with the build server. If you want to run
            abrasive builds then this is required.
          </p>
          <p>
            To prevent keys being silently leaked they are stored on abrasive in
            hashed form. This means you can only download keys when you first
            create them. If you have old unused keys you can safely delete them
            and create a new one.
          </p>
          <p>
            To use an API token, run <code>abrasive auth</code> on the command
            line and paste the key when prompted. This will save it to a local
            credentials file. For CI systems you can use the{' '}
            <code>ABRASIVE_TOKEN</code> environment variable, but make sure that
            the token stays secret!
          </p>
        </div>

        {showForm && (
          <form className="token-form" onSubmit={onCreate}>
            <label className="field">
              <span>Name</span>
              <input
                type="text"
                placeholder="e.g. work-laptop"
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
                disabled={busy}
              />
            </label>
            <label className="field">
              <span>Scope</span>
              <select
                value={scope}
                onChange={(e) => setScope(e.target.value as Scope)}
                disabled={busy}
              >
                <option value="read-write">read-write</option>
                <option value="read">read</option>
              </select>
            </label>
            <label className="field">
              <span>Expires</span>
              <select
                value={expirationDays === null ? 'never' : String(expirationDays)}
                onChange={(e) =>
                  setExpirationDays(e.target.value === 'never' ? null : Number(e.target.value))
                }
                disabled={busy}
              >
                {EXPIRATION_OPTIONS.map((o) => (
                  <option key={o.label} value={o.days === null ? 'never' : String(o.days)}>
                    {o.label}
                  </option>
                ))}
              </select>
            </label>
            <div className="form-actions">
              <button type="submit" className="primary-btn" disabled={busy || !name.trim()}>
                {busy ? 'Creating…' : 'Create'}
              </button>
              <button
                type="button"
                className="link-btn"
                onClick={() => setShowForm(false)}
                disabled={busy}
              >
                Cancel
              </button>
            </div>
          </form>
        )}

        {error && <div className="error">{error}</div>}

        {tokens === null ? (
          <p className="muted">Loading…</p>
        ) : tokens.length === 0 ? (
          <p className="muted">No tokens yet.</p>
        ) : (
          <ul className="token-list">
            {tokens.map((t) => {
              const isNew = justCreated?.row.id === t.id
              return (
                <li key={t.id} className="token-card">
                  <div className="token-card-header">
                    <h3>{t.name}</h3>
                  </div>
                  <div className="token-card-meta">
                    <span>Scope: {t.scope}</span>
                    <span>
                      {t.last_used_at
                        ? `Last used ${relativeTimePast(t.last_used_at)} ago`
                        : 'Never used'}
                    </span>
                    <span>Created {relativeTimePast(t.created_at)} ago</span>
                    <span>{expirationText(t.expires_at)}</span>
                  </div>

                  {isNew && justCreated && (
                    <div className="just-created">
                      <p>
                        <strong>Make sure to copy your API token now.</strong>{' '}
                        You won't be able to see it again!
                      </p>
                      <CodeBlock code={justCreated.plaintext} wrap />
                    </div>
                  )}

                  <div className="token-card-actions">
                    <button
                      className="primary-btn"
                      onClick={() => onRegenerate(t)}
                      disabled={busy}
                    >
                      Regenerate
                    </button>
                    <button
                      className="danger-btn"
                      onClick={() => onDelete(t)}
                      disabled={busy}
                    >
                      Revoke
                    </button>
                  </div>
                </li>
              )
            })}
          </ul>
        )}
      </main>
      <Footer />
    </div>
  )
}
