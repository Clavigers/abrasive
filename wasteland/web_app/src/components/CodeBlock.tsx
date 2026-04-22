import { useState } from 'react'
import { showToast } from '../lib/toast'

type Props = { code: string; wrap?: boolean }

const CopyIcon = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
    <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25z"/>
    <path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25z"/>
  </svg>
)

const CheckIcon = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="#3fb950">
    <path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z"/>
  </svg>
)

export default function CodeBlock({ code, wrap }: Props) {
  const [copied, setCopied] = useState(false)

  const onClick = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    showToast('Copied to clipboard!')
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <button
      className={wrap ? 'code-block code-block-wrap' : 'code-block'}
      onClick={onClick}
      aria-label={`Copy: ${code}`}
    >
      <code>{code}</code>
      <span className="copy-icon" aria-hidden="true">
        {copied ? <CheckIcon /> : <CopyIcon />}
      </span>
    </button>
  )
}
