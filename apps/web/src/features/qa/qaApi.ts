import { request } from '../../lib/api'
export interface QaActor { label: string; token?: string | null; prefix?: string }
export interface QaAssertion { path: string; op: string; value?: unknown }
export interface QaCall { id: string; name: string; method: string; path: string; actor: string; body: unknown; extracts: Record<string, string>; assertions: QaAssertion[]; encrypt: string[] }
export interface QaAction { id: string; title: string; description: string; calls: QaCall[] }
export interface QaScenario { id: string; module: string; title: string; purpose: string; uses: string[]; actions: QaAction[] }
export interface QaSuite { name: string; baseUrl: string; siteUrl: string; publicKey: string; actors: Record<string, QaActor>; variables: Record<string, unknown>; scenarios: QaScenario[] }
export interface QaResult { callId: string; scenarioId: string; name: string; actor: string; status: string; httpStatus: number | null; elapsedMs: number; request: unknown; response: unknown; checks: { name: string; passed: boolean }[]; error: string | null }
export interface QaRun { id: string; scenarioId: string; suiteRevision: number; status: string; createdAt: number; updatedAt: number; accountName: string; currentCall: string | null; results: QaResult[]; variables: Record<string, unknown>; error: string | null }
export interface QaSnapshot { workspaceKey: string; revision: number; suite: QaSuite | null; runs: QaRun[] }
export type QaOperation = { type: 'seed' } | { type: 'save'; suite: QaSuite } | { type: 'run'; scenarioId: string; inputs: Record<string, string> } | { type: 'cancel'; runId: string }
export function getQa(workspaceKey: string, expectedRevision: number, signal?: AbortSignal) { return request<QaSnapshot>(`/api/qa?${new URLSearchParams({ workspaceKey, expectedRevision: String(expectedRevision) })}`, { signal }) }
export function executeQa(workspaceKey: string, expectedRevision: number, qaRevision: number, operation: QaOperation) { return request<QaSnapshot>('/api/qa', { method: 'POST', body: JSON.stringify({ workspaceKey, expectedRevision, qaRevision, operation }) }) }
export function qaExecutionPlan(scenario: QaScenario, scenarios: QaScenario[]): QaScenario[] {
 const seen = new Set<string>(), plan: QaScenario[] = []
 function visit(item: QaScenario) { if (seen.has(item.id)) return; seen.add(item.id); for (const id of item.uses) { const dependency = scenarios.find(s => s.id === id); if (dependency) visit(dependency) } plan.push(item) }
 visit(scenario); return plan
}
