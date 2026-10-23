// Real, repeatable acceptance through DevFlow's HTTP/MCP adapters.
// node scripts/verify-qa-extreme.cjs --run [--mcp] creates one test account.
// Without --run, validates the latest persisted successful execution only.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const root = 'http://127.0.0.1:31080';
const runRequested = process.argv.includes('--run');
const useMcp = process.argv.includes('--mcp');
(async () => {
 const session = await fetch(root + '/api/session', { headers: { 'X-DevFlow-Client': 'web' } }).then(r => r.json());
 const headers = { Authorization: `Bearer ${session.token}`, 'Content-Type': 'application/json' };
 async function api(path, body) {
  const response = await fetch(root + path, { headers, method: body ? 'POST' : 'GET', body: body && JSON.stringify(body) });
  const data = await response.json(); if (!response.ok) throw Error(`${response.status}: ${data.message}`); return data;
 }
 async function mcp(name, args) {
  const reply = await api('/mcp', { jsonrpc: '2.0', id: Date.now(), method: 'tools/call', params: { name, arguments: args } });
  assert(!reply.error, JSON.stringify(reply.error));
  const result = reply.result; assert(!result.isError, result.content?.[0]?.text);
  return JSON.parse(result.content[0].text);
 }
 const ws = await api('/api/projects');
 const scope = { workspaceKey: ws.workspaceFile || ws.workspaceRoot, expectedRevision: ws.revision };
 assert(scope.workspaceKey, 'Open a workspace first');
 const query = '/api/qa?' + new URLSearchParams(scope);
 const read = () => useMcp ? mcp('qa_get', scope) : api(query);
 const mutate = (snapshot, operation) => useMcp ? mcp('qa_execute', { ...scope, qaRevision: snapshot.revision, operation }) : api('/api/qa', { ...scope, qaRevision: snapshot.revision, operation });
 let snapshot = await read();
 if (!snapshot.suite && runRequested) snapshot = await mutate(snapshot, { type: 'seed' });
 assert(snapshot.suite, 'Initialize the Extreme example first, or pass --run');
 assert.equal(snapshot.suite.baseUrl, 'http://192.168.1.200/prod-api/system/');
 let id;
 if (runRequested) {
  snapshot = await mutate(snapshot, { type: 'run', scenarioId: 'account-profile', inputs: { adminPassword: process.env.DEVFLOW_QA_ADMIN_PASSWORD || 'admin' } });
  id = snapshot.runs[0].id; console.log(`Started ${id}, account ${snapshot.runs[0].accountName}`);
  const deadline = Date.now() + 120000;
  while (['running', 'cancelling'].includes(snapshot.runs.find(r => r.id === id)?.status)) {
   assert(Date.now() < deadline, 'Acceptance timed out');
   await new Promise(resolve => setTimeout(resolve, 500)); snapshot = await read();
  }
 } else id = snapshot.runs.find(r => r.scenarioId === 'account-profile' && r.status === 'passed')?.id;
 const run = snapshot.runs.find(r => r.id === id);
 assert(run, 'No successful full acceptance record'); assert.equal(run.status, 'passed', JSON.stringify(run.results.filter(s => s.status !== 'passed')));
 assert.equal(run.results.length, 8);
 for (const step of run.results) { assert.equal(step.status, 'passed', step.name); assert.equal(step.httpStatus, 200); assert(step.checks.every(c => c.passed), step.name); }
 const info = run.results.find(s => s.callId === 'new-info').response;
 assert.equal(info.data.username, run.accountName); assert.equal(String(info.data.id), run.variables.accountId);
 assert(info.data.roles.includes('cs_role_3035')); assert(!info.data.roles.includes('admin'));
 for (const name of ['adminPassword', 'adminToken', 'actorAToken', 'initialPassword']) assert.equal(run.variables[name], '[REDACTED]');
 for (const name of ['admin-login-call', 'new-login']) { const result = run.results.find(s => s.callId === name); assert.equal(result.response.data, '[REDACTED]'); assert.equal(result.request.body.password, '[REDACTED]'); }
 const mcpRead = await mcp('qa_get', scope); assert(mcpRead.runs.some(r => r.id === run.id), 'MCP and HTTP state must match');
 const unauthorized = await fetch(root + query); assert.equal(unauthorized.status, 401);
 const stale = await fetch(root + '/api/qa', { method: 'POST', headers, body: JSON.stringify({ ...scope, expectedRevision: ws.revision + 1, qaRevision: snapshot.revision, operation: { type: 'run', scenarioId: 'account-profile', inputs: { adminPassword: 'unused' } } }) }); assert.equal(stale.status, 409);
 const conflict = await fetch(root + '/api/qa', { method: 'POST', headers, body: JSON.stringify({ ...scope, qaRevision: snapshot.revision + 1, operation: { type: 'seed' } }) }); assert.equal(conflict.status, 409);
 const badSuite = structuredClone(snapshot.suite); badSuite.variables.adminPassword = 'do-not-persist';
 const rejected = await fetch(root + '/api/qa', { method: 'POST', headers, body: JSON.stringify({ ...scope, qaRevision: snapshot.revision, operation: { type: 'save', suite: badSuite } }) }); assert.equal(rejected.status, 400);
 fs.mkdirSync('.devflow/qa-validation', { recursive: true });
 fs.writeFileSync('.devflow/qa-validation/accepted-run.json', JSON.stringify(run, null, 2));
 const evidence = { runId: run.id, workspaceKey: scope.workspaceKey, accountName: run.accountName, accountId: run.variables.accountId, roleCode: 'cs_role_3035', status: run.status, checks: run.results.reduce((n, s) => n + s.checks.length, 0), steps: run.results.map(s => ({ name: s.name, actor: s.actor, httpStatus: s.httpStatus, elapsedMs: s.elapsedMs, checks: s.checks.length })), verification: ['HTTP/MCP shared state', '401 without owner token', '409 stale workspace', '409 stale QA revision', '400 literal credentials refused', 'password/token redaction'] };
 fs.writeFileSync('.devflow/qa-validation/acceptance-summary.json', JSON.stringify(evidence, null, 2));
 console.log(JSON.stringify(evidence, null, 2));
})().catch(error => { console.error(error.message); process.exitCode = 1; });
