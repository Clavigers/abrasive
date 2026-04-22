import { useEffect, useRef, useState } from 'react'
import type { Session } from '@supabase/supabase-js'
import { supabase } from './lib/supabase'

type Props = { session: Session }

export default function TopBar({ session }: Props) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!menuOpen) return
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', onClick)
    return () => document.removeEventListener('mousedown', onClick)
  }, [menuOpen])

  const meta = session.user.user_metadata as Record<string, unknown>
  const avatarUrl = meta.avatar_url as string | undefined
  const username =
    (meta.user_name as string | undefined) ||
    (meta.preferred_username as string | undefined) ||
    session.user.email ||
    session.user.id
  const initial = (username[0] || '?').toUpperCase()

  return (
    <header className="topbar">
      <div className="topbar-left">
        <span className="topbar-brand">abrasive</span>
      </div>
      <div className="topbar-right" ref={menuRef}>
        <button
          className="avatar-btn"
          onClick={() => setMenuOpen((v) => !v)}
          aria-label="Account menu"
          aria-expanded={menuOpen}
        >
          {avatarUrl ? (
            <img className="avatar-img" src={avatarUrl} alt="" />
          ) : (
            <span className="avatar-fallback">{initial}</span>
          )}
        </button>
        {menuOpen && (
          <div className="avatar-menu" role="menu">
            <div className="avatar-menu-header">
              <div className="avatar-menu-username">@{username}</div>
              {session.user.email && (
                <div className="avatar-menu-email">{session.user.email}</div>
              )}
            </div>
            <button
              className="avatar-menu-item"
              role="menuitem"
              onClick={() => supabase.auth.signOut()}
            >
              Sign out
            </button>
          </div>
        )}
      </div>
    </header>
  )
}
