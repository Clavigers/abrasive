import { useEffect } from 'react'
import type { Session } from '@supabase/supabase-js'
import { navigate } from '../router'

type Props = { session: Session | null }

export default function Me({ session }: Props) {
  useEffect(() => {
    if (session) navigate('/settings/tokens')
    else navigate('/sign-in?next=/settings/tokens')
  }, [session])

  return <main className="gate"><p>Redirecting…</p></main>
}
