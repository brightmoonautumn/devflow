"""Isolated real Node/npm acceptance; never changes the user's package/workspace files."""
import argparse,json,os,pathlib,subprocess,time,urllib.request,urllib.error,socket,sys
sys.stdout.reconfigure(encoding='utf-8');sys.stderr.reconfigure(encoding='utf-8')
urllib.request.install_opener(urllib.request.build_opener(urllib.request.ProxyHandler({})))
ROOT=pathlib.Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser();parser.add_argument('--real-project',action='store_true');parser.add_argument('--backend',default='target/node-runtime/debug/devflow.exe');parser.add_argument('--keep-backend',action='store_true');parser.add_argument('--port',type=int,default=31081)
opt=parser.parse_args()
base=f'http://127.0.0.1:{opt.port}'
folder=ROOT/'.devflow'/'acceptance'/f'Node 中文 空格 {int(time.time())}'
folder.mkdir(parents=True);multi=folder/'聚合 项目';multi.mkdir();standalone=folder/'普通 前端';standalone.mkdir()
def write(path,value):path.write_text(value,encoding='utf-8')
pkg={'scripts':{'serve':'node server.cjs','system':'npm run serve --project=system','integration':'npm run serve --project=integration','failure':'npm run serve --project=failure','build':'echo do-not-launch','test':'echo do-not-launch'}}
write(multi/'package.json',json.dumps(pkg,ensure_ascii=False))
server=r"""const http=require('node:http'),{spawn}=require('node:child_process');
const name=process.env.npm_config_project||'standalone';
if(name==='failure'){console.error('失败样例 中文');process.exit(7)}
const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore',windowsHide:true});
console.log('CHILD:'+child.pid);console.error('stderr 中文');
const server=http.createServer((req,res)=>{if(req.url==='/emit'){for(let i=0;i<2100;i++)console.log('LINE:'+i);console.log('L'.repeat(20000))}res.end(name)});
server.listen(0,'127.0.0.1',()=>console.log('READY:'+JSON.stringify({name,pid:process.pid,child:child.pid,port:server.address().port,args:process.argv.slice(2),label:process.env.npm_config_label||''})));
"""
write(multi/'server.cjs',server);write(standalone/'server.cjs',server);write(standalone/'package.json',json.dumps({'scripts':{'dev':'node server.cjs','build':'echo ignored'}}))
real=pathlib.Path('D:/CompanyProject/main-system')
workspace=folder/'node.code-workspace';write(workspace,json.dumps({'folders':[{'path':str(multi)},{'path':str(standalone)}]+([{'path':str(real)}] if real.is_dir() else [])}))
env=os.environ.copy();env.update(DEVFLOW_PORT=str(opt.port),DEVFLOW_WORKSPACE=str(workspace),DEVFLOW_ALLOWED_ROOTS=os.pathsep.join([str(folder)]+([str(real)] if real.is_dir() else [])))
for manager,entry in [('PNPM','pnpm/bin/pnpm.mjs'),('YARN','yarn/bin/yarn.js')]:
 candidate=ROOT/'.devflow'/'acceptance'/'node-tools'/'node_modules'/entry
 if candidate.is_file():env['DEVFLOW_'+manager+'_CLI']=str(candidate)
log=open(folder/'backend.log','wb');proc=None;token='';checks=[];started=[]
def call(path,body=None,status=200,auth=True):
 data=None if body is None else json.dumps(body,ensure_ascii=False).encode('utf-8')
 headers={'Content-Type':'application/json'}
 if auth and token:headers['Authorization']='Bearer '+token
 if path=='/api/session':headers['X-DevFlow-Client']='web'
 request=urllib.request.Request(base+path,data=data,headers=headers)
 try:
  with urllib.request.urlopen(request,timeout=35) as r:code=r.status;value=json.load(r)
 except urllib.error.HTTPError as e:code=e.code;value=json.loads(e.read())
 assert code==status,(path,code,value)
 return value

def check(name,condition):
 assert condition,name
 checks.append(name);print('PASS',name,flush=True)

def mcp(name,args):
 result=call('/mcp',{'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':name,'arguments':args}})['result']
 if result.get('isError'):return {'error':json.loads(result['content'][0]['text'])}
 return result.get('structuredContent',json.loads(result['content'][0]['text']))

def request(project,script=None,operation=None,revision=None):return {'projectPath':str(project),**({'script':script} if script else {}),'expectedRevision':rev if revision is None else revision,'operation':operation or {'type':'start','packageManager':'npm','args':[]}}
def output(project,script=None,**query):
 from urllib.parse import urlencode
 return call('/api/runtime/node/output?'+urlencode({'projectPath':str(project),**({'script':script} if script else {}),**query}))
def wait_for(project,script,predicate,seconds=35):
 deadline=time.time()+seconds
 while time.time()<deadline:
  value=output(project,script)
  if predicate(value):return value
  if value['state']=='failed':raise AssertionError(value)
  time.sleep(.2)
 raise AssertionError(('timed out',value))
def ready(project,script=None):
 value=wait_for(project,script,lambda v:any(line['text'].startswith('READY:') for line in v['lines']))
 data=json.loads(next(line['text'][6:] for line in value['lines'] if line['text'].startswith('READY:')))
 return value,data

def metrics(project,script=None):
 value=call('/api/runtime/projects')
 return next(m for m in value['projects'] if pathlib.Path(m['projectPath']).resolve()==project.resolve() and m.get('nodeScript')==script)
def alive(pid):
 if os.name!='nt':
  try:os.kill(pid,0);return True
  except ProcessLookupError:return False
 import ctypes
 kernel=ctypes.WinDLL('kernel32',use_last_error=True);kernel.OpenProcess.restype=ctypes.c_void_p;kernel.OpenProcess.argtypes=[ctypes.c_uint32,ctypes.c_int,ctypes.c_uint32];kernel.CloseHandle.argtypes=[ctypes.c_void_p];kernel.GetExitCodeProcess.argtypes=[ctypes.c_void_p,ctypes.POINTER(ctypes.c_uint32)]
 handle=kernel.OpenProcess(0x1000,False,pid)
 if not handle:return False
 code=ctypes.c_uint32();ok=kernel.GetExitCodeProcess(handle,ctypes.byref(code));kernel.CloseHandle(handle);return bool(ok and code.value==259)
def stop(project,script=None):
 return call('/api/runtime/node',request(project,script,{'type':'stop'}))
try:
 try:original_frontend=urllib.request.urlopen('http://127.0.0.1:8081',timeout=3).status==200
 except Exception:original_frontend=False
 with socket.socket() as s:
  try:s.bind(('127.0.0.1',opt.port))
  except OSError:raise RuntimeError('Acceptance port already in use')
 proc=subprocess.Popen([str((ROOT/opt.backend).resolve())],cwd=ROOT,env=env,stdout=log,stderr=log,creationflags=0x08000000 if os.name=='nt' else 0)
 for _ in range(100):
  try:
   if call('/api/health',auth=False)['status']=='ok':break
  except Exception:
   if proc.poll() is not None:raise RuntimeError((folder/'backend.log').read_text(encoding='utf-8'))
   time.sleep(.1)
 token=call('/api/session',auth=False)['token'];ws=call('/api/projects');rev=ws['revision']
 entries=[p for p in ws['projects'] if pathlib.Path(p['path']).resolve()==multi.resolve()]
 check('aggregate scripts become three independent services',{p['nodeScript'] for p in entries}=={'system','integration','failure'})
 check('standalone dev discovered',any(pathlib.Path(p['path']).resolve()==standalone.resolve() for p in ws['projects']))
 if real.is_dir():
  real_entries=[p for p in ws['projects'] if pathlib.Path(p['path']).resolve()==real.resolve()]
  expected={'system','integration','file','notice','auth','form','meta','admin','gateway','extension','ops','ai','system-old'}
  check('real main-system all 13 aliases discovered',{p['nodeScript'] for p in real_entries}==expected)
  check('HTTP and MCP project discovery agree',mcp('workspace_get',{})['projects']==ws['projects'])
 call('/api/runtime/node',request(multi,'system'),status=401,auth=False);check('mutation requires authentication',True)
 call('/api/runtime/node',request(multi,'system',revision=rev+1),status=409);check('stale revision rejected',True)
 call('/api/runtime/node',request(multi,'build'),status=403);check('build script cannot be launched',True)
 call('/api/runtime/node',request(ROOT,'dev'),status=403);check('outside workspace rejected',True)
 start=call('/api/runtime/node',request(multi,'system',{'type':'start','packageManager':'npm','args':['--label=中文 空格']}));started.append((multi,'system'))
 check('start responds asynchronously',start['state']=='starting')
 first,sysinfo=ready(multi,'system');check('UTF8/spaced paths and forwarded npm project selector',sysinfo['name']=='system' and sysinfo['label']=='中文 空格')
 check('stdout stderr captured',any(line['stream']=='stderr' and 'stderr 中文' in line['text'] for line in first['lines']))
 duplicate=mcp('node_execute',request(multi,'system'));check('MCP cannot duplicate HTTP start',duplicate.get('error',{}).get('code')=='conflict')
 call('/api/runtime/node',request(multi,'system',{'type':'stop','pid':os.getpid()}),status=409);check('unrelated PID rejected even while managed',True)
 result=mcp('node_execute',request(multi,'integration'));check('MCP launches second alias',result['state']=='starting');started.append((multi,'integration'))
 second,intinfo=ready(multi,'integration');check('sibling script parameters isolated',intinfo['name']=='integration')
 a=metrics(multi,'system');b=metrics(multi,'integration');check('sibling metrics PIDs isolated',bool(a['pids']) and bool(b['pids']) and not set(a['pids'])&set(b['pids']))
 check('ports follow individual services',sysinfo['port'] in a['listeningPorts'] and intinfo['port'] in b['listeningPorts'] and intinfo['port'] not in a['listeningPorts'])
 check('child process included in managed tree',sysinfo['child'] in a['pids'])
 delta=output(multi,'system',after=first['nextCursor'],runId=first['runId']);check('incremental cursor avoids replay',not delta['lines'])
 result=mcp('node_execute',request(multi,'system',{'type':'stop'}));check('MCP stops HTTP launch',result['state']=='stopped')
 for _ in range(30):
  remaining=metrics(multi,'system')['pids']
  if not remaining:break
  time.sleep(.2)
 check('stopped service tree and ports cleared',not remaining and not metrics(multi,'system')['listeningPorts'])
 check('all stopped root and child PIDs exited in OS',not any(alive(pid) for pid in a['pids']))
 with urllib.request.urlopen(f"http://127.0.0.1:{intinfo['port']}") as r:check('stopping one alias preserves sibling',r.read()==b'integration')
 call('/api/runtime/node',request(multi,'system'));third,_=ready(multi,'system');check('new run resets cursor',third['runId']!=first['runId']);stop(multi,'system')
 stop(multi,'integration')
 call('/api/runtime/node',request(multi,'failure'));started.append((multi,'failure'))
 deadline=time.time()+20
 while time.time()<deadline:
  failed=output(multi,'failure')
  if failed['state']=='failed':break
  time.sleep(.2)
 check('nonzero exit becomes failed with output',failed['state']=='failed' and failed['exitCode']!=0 and any('失败样例' in l['text'] for l in failed['lines']))
 call('/api/runtime/node',request(standalone));started.append((standalone,None));plain,plaininfo=ready(standalone)
 check('standalone default dev launch',plaininfo['name']=='standalone')
 with urllib.request.urlopen(f"http://127.0.0.1:{plaininfo['port']}/emit") as r:r.read()
 time.sleep(1);bounded=output(standalone,after=0,runId=plain['runId'])
 check('output bounds and cursor gap',bounded['truncated'] and len(bounded['lines'])==500 and all(len(l['text'].encode('utf-8'))<=16384 for l in bounded['lines']))
 stop(standalone)
 for manager in ['pnpm','yarn']:
  if 'DEVFLOW_'+manager.upper()+'_CLI' not in env:continue
  tool=json.loads((ROOT/'.devflow'/'acceptance'/'node-tools'/'node_modules'/manager/'package.json').read_text(encoding='utf-8'))
  write(standalone/'package.json',json.dumps({'packageManager':manager+'@'+tool['version'],'scripts':{'dev':'node server.cjs'}}))
  call('/api/runtime/node',request(standalone,operation={'type':'start','packageManager':manager,'args':['--value=中文 空格']}))
  managed,info=ready(standalone)
  check(manager+' real start and literal spaced arguments', '--value=中文 空格' in info['args'])
  stop(standalone)
 call('/api/runtime/node',request(multi,'system'));stop(multi,'system');time.sleep(1)
 check('startup cancellation leaves no live managed run',output(multi,'system')['state']=='stopped' and not metrics(multi,'system')['pids'])
 original=(multi/'package.json').read_text(encoding='utf-8');edited=dict(pkg);edited['scripts']=dict(pkg['scripts']);edited['scripts'].pop('system');write(multi/'package.json',json.dumps(edited))
 call('/api/runtime/node',request(multi,'system'),status=400);write(multi/'package.json',original);check('script is revalidated before every launch',True)
 tools=mcp('node_options',{'projectPath':str(multi),'script':'integration'});check('MCP options use shared service whitelist',tools['scripts']==['integration'])
 if opt.real_project and real.is_dir():
  script='file'
  call('/api/runtime/node',request(real,script));started.append((real,script))
  real_output=wait_for(real,script,lambda value:any('Local:' in line['text'] or 'ready in' in line['text'] for line in value['lines']),seconds=100)
  deadline=time.time()+15
  while time.time()<deadline:
   live=metrics(real,script)
   if live.get('listeningPorts'):break
   time.sleep(.3)
  ports=live.get('listeningPorts') or []
  check('real main-system/file Vite starts with listening port',bool(ports))
  for port in ports:
   try:
    with urllib.request.urlopen(f'http://127.0.0.1:{port}',timeout=15) as response:
     html=response.read().decode('utf-8');assert '<html' in html or '<!doctype' in html.lower();break
   except Exception:continue
  else:raise AssertionError('real Vite HTTP did not respond')
  check('real Vite serves frontend HTML',True)
  write(folder/'real-project.json',json.dumps({'ports':ports,'pids':live['pids'],'output':real_output},ensure_ascii=False,indent=2))
  stop(real,script)
  time.sleep(.5)
  check('real Vite process tree exits',not any(alive(pid) for pid in live['pids']))
 if original_frontend:
  last_error=None
  for _ in range(15):
   try:
    with urllib.request.urlopen('http://127.0.0.1:8081',timeout=5) as response:
     if response.status==200:last_error=None;break
   except Exception as err:last_error=err
   time.sleep(1)
  check('original frontend still reachable',last_error is None)

 print('OK',len(checks),'checks',flush=True)
finally:
 for project,script in started:
  try:stop(project,script)
  except Exception:pass
 report={'checks':checks,'count':len(checks),'base':base,'backendPid':proc.pid if proc else None,'fixture':str(folder),'workspace':str(workspace)}
 write(folder/'result.json',json.dumps(report,ensure_ascii=False,indent=2));write(ROOT/'.devflow'/'acceptance'/'node-latest.json',json.dumps(report,ensure_ascii=False,indent=2))
 if proc and not opt.keep_backend:proc.terminate();proc.wait(timeout=10)
 log.close()
 print('Evidence:',folder,flush=True)
