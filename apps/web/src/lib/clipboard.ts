/** Clipboard API needs HTTPS/loopback; LAN HTTP keeps the same copy action. */
export async function copyText(value: string) {
  if (navigator.clipboard) {
    try { await navigator.clipboard.writeText(value); return } catch { /* Fall back to the user-initiated copy command. */ }
  }
  const previous = document.activeElement as HTMLElement | null
  const input = document.createElement('textarea')
  input.value = value; input.readOnly = true
  input.style.cssText = 'position:fixed;left:-9999px;top:0;opacity:0'
  // Native dialogs make siblings inert, so mount in the active dialog if any.
  ;(document.querySelector('dialog[open]') ?? document.body).append(input)
  try { input.select(); if (!document.execCommand('copy')) throw new Error('请手动复制文本。') }
  finally { input.remove(); previous?.focus({ preventScroll: true }) }
}
