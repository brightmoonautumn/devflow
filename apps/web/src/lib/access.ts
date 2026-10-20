/** Both entry points mount App; only the capability and transport differ. */
export const readOnlyShare = location.pathname.startsWith('/share/')
function loadInvitation() {
  if (!readOnlyShare) return ''
  const fragment = location.hash.slice(1)
  const valid = /^[a-f0-9]{64}$/.test(fragment)
  try {
    if (valid) sessionStorage.setItem('devflow-readonly-invitation', fragment)
    return valid ? fragment : sessionStorage.getItem('devflow-readonly-invitation') ?? ''
  } catch { return valid ? fragment : '' }
  finally { if (location.hash) history.replaceState(null, '', location.pathname) }
}
export const invitation = loadInvitation()
export function expireInvitation(message: string) {
  try { sessionStorage.removeItem('devflow-readonly-invitation') } catch { /* Storage is optional. */ }
  window.dispatchEvent(new CustomEvent('devflow-share-expired', { detail: message }))
}
