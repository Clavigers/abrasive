import { useEffect, useState } from 'react'
import type { Session } from '@supabase/supabase-js'
import { supabase } from './lib/supabase'
import { useRoute, navigate } from './router'
import TopBar from './TopBar'
import SignIn from './pages/SignIn'
import Me from './pages/Me'
import Tokens from './pages/Tokens'

export default function App() {
  const [session, setSession] = useState<Session | null>(null)
  const [loading, setLoading] = useState(true)
  const path = useRoute()

  useEffect(() => {
    supabase.auth.getSession().then(({ data }) => {
      setSession(data.session)
      setLoading(false)
    })
    const { data: sub } = supabase.auth.onAuthStateChange((_event, s) => setSession(s))
    return () => sub.subscription.unsubscribe()
  }, [])

  if (loading) return <main className="gate"><p>Loading…</p></main>

  if (path === '/sign-in') return <SignIn />
  if (path === '/me') return <Me session={session} />

  if (path === '/settings/tokens') {
    if (!session) {
      navigate('/sign-in?next=/settings/tokens')
      return <main className="gate"><p>Redirecting…</p></main>
    }
    return <Tokens session={session} />
  }

  if (!session) return <SignIn />

  const meta = session.user.user_metadata as Record<string, unknown>
  const name =
    (meta.user_name as string | undefined) ||
    (meta.preferred_username as string | undefined) ||
    session.user.email ||
    session.user.id

  return (
    <div className="app">
      <TopBar session={session} />
      <main className="dash">
        <h1>Hello, {name}</h1>
        <a className="link-btn" href="/settings/tokens" onClick={(e) => { e.preventDefault(); navigate('/settings/tokens') }}>
          API tokens →
        </a>
      </main>
    </div>
  )
}
