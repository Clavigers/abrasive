import { useEffect, useState } from 'react'

const BASE = import.meta.env.BASE_URL.replace(/\/$/, '')

const toLogical = (p: string) =>
  BASE && p.startsWith(BASE) ? p.slice(BASE.length) || '/' : p

const toPhysical = (p: string) => (BASE ? BASE + p : p)

export function useRoute() {
  const [path, setPath] = useState(toLogical(window.location.pathname))
  useEffect(() => {
    const onPop = () => setPath(toLogical(window.location.pathname))
    window.addEventListener('popstate', onPop)
    window.addEventListener('abrasive:navigate', onPop)
    return () => {
      window.removeEventListener('popstate', onPop)
      window.removeEventListener('abrasive:navigate', onPop)
    }
  }, [])
  return path
}

export function navigate(to: string) {
  const physical = toPhysical(to)
  if (physical === window.location.pathname + window.location.search) return
  window.history.pushState({}, '', physical)
  window.dispatchEvent(new Event('abrasive:navigate'))
}

export const href = (logical: string) => toPhysical(logical)
