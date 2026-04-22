let toast: HTMLDivElement | null = null
let timer: number | undefined

export function showToast(message: string) {
  if (!toast) {
    toast = document.createElement('div')
    toast.className = 'copy-toast'
    document.body.appendChild(toast)
  }
  toast.textContent = message
  requestAnimationFrame(() => toast!.classList.add('visible'))
  clearTimeout(timer)
  timer = window.setTimeout(() => toast!.classList.remove('visible'), 1500)
}
