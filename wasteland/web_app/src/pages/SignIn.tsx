import { useEffect, useState } from 'react'
import type { Session } from '@supabase/supabase-js'
import { supabase } from '../lib/supabase'
import { navigate } from '../router'
import TopBar from '../TopBar'
import cuddlyferris from '../assets/cuddlyferris.svg'

export default function SignIn() {
  const [checked, setChecked] = useState(false)
  const [session, setSession] = useState<Session | null>(null)

  useEffect(() => {
    supabase.auth.getSession().then(({ data }) => {
      setSession(data.session)
      setChecked(true)
    })
  }, [])

  useEffect(() => {
    if (!checked) return
    if (session) {
      const next = new URLSearchParams(window.location.search).get('next') || '/'
      navigate(next)
    }
  }, [checked, session])

  const signIn = () => {
    const next = new URLSearchParams(window.location.search).get('next')
    const redirectTo = window.location.origin + (next || '/dashboard')
    supabase.auth.signInWithOAuth({
      provider: 'github',
      options: { redirectTo },
    })
  }

  if (!checked) return <main className="gate"><p>Loading…</p></main>
  if (session) return <main className="gate"><p>Redirecting…</p></main>

  return (
    <div className="app">
      <TopBar session={null} />
      <main className="auth-wrapper">
        <div className="auth-content">
          <img src={cuddlyferris} alt="" className="auth-logo" />
          <h1 className="auth-title">This page requires authentication</h1>
          <button className="auth-link" onClick={signIn}>
            Log in with GitHub
          </button>
        </div>
      </main>
    </div>
  )
}
