import { request } from '../../lib/api'
export type Invitation = { id: string; name: string; folders: string[]; expiresAt: number }
export type ShareCatalog = { network: { address: string | null; suggestedUrl: string | null; error: string | null }; invitations: Invitation[] }
export const listShares = () => request<ShareCatalog>('/api/workspace/shares')
export const createShare = (expectedRevision: number, lifetimeMinutes: number) => request<{ invitation: Invitation; token: string }>('/api/workspace/shares', { method: 'POST', body: JSON.stringify({ expectedRevision, lifetimeMinutes }) })
export const revokeShare = (id: string) => request('/api/workspace/shares/revoke', { method: 'POST', body: JSON.stringify({ id }) })
