export type QaCall = { id: string; name: string; method: 'GET' | 'POST' | 'DELETE'; path: string; body?: object; extract?: string; expect: string; response: object }
export type QaAction = { id: string; title: string; description: string; calls: QaCall[] }
export type QaScenario = { id: string; module: string; title: string; purpose: string; actions: QaAction[] }

const listCall: QaCall = { id: 'list', name: '加载用户列表', method: 'GET', path: '/api/system/users?page=1&size=20', expect: '列表加载成功，返回分页数据', response: { code: 0, data: { total: 12, records: [] } } }
export const qaScenarios: QaScenario[] = [
  { id: 'create', module: '用户管理', title: '管理员正常添加用户', purpose: '管理员创建一名普通用户，并在列表中确认用户已生效。', actions: [
    { id: 'enter', title: '进入用户管理页', description: '打开管理页，加载列表及当前操作权限。', calls: [listCall, { id: 'permissions', name: '获取操作权限', method: 'GET', path: '/api/system/users/permissions', expect: '管理员拥有新增用户权限', response: { code: 0, data: ['user:create', 'user:read'] } }] },
    { id: 'open', title: '点击「新增用户」', description: '打开表单，加载角色和部门选项。', calls: [{ id: 'roles', name: '加载可选角色', method: 'GET', path: '/api/system/roles/options', expect: '可选角色包含普通用户', response: { code: 0, data: [{ id: 'role_user', name: '普通用户' }] } }, { id: 'departments', name: '加载部门树', method: 'GET', path: '/api/system/departments/tree', expect: '部门树正常返回', response: { code: 0, data: [{ id: 'dept_01', name: '研发部' }] } }] },
    { id: 'submit', title: '填写并提交用户', description: '填写用户名、姓名和角色，保存后提取新用户 ID。', calls: [{ id: 'create-user', name: '创建用户', method: 'POST', path: '/api/system/users', body: { username: 'qa_demo_zhang', name: '张小明', roleId: 'role_user', departmentId: 'dept_01' }, extract: 'userId ← $.data.id', expect: '业务码为 0，返回新用户 ID', response: { code: 0, data: { id: 'usr_1024', username: 'qa_demo_zhang' } } }] },
    { id: 'verify', title: '确认新增结果', description: '刷新列表并查询详情，确认新用户信息与表单一致。', calls: [{ ...listCall, id: 'refresh', name: '刷新用户列表', path: '/api/system/users?username={{username}}', expect: '列表中出现刚创建的用户', response: { code: 0, data: { total: 1, records: [{ id: 'usr_1024', name: '张小明' }] } } }, { id: 'detail', name: '核对用户详情', method: 'GET', path: '/api/system/users/{{userId}}', expect: '姓名、角色、部门与提交值一致', response: { code: 0, data: { id: 'usr_1024', name: '张小明', roleId: 'role_user', departmentId: 'dept_01' } } }] },
    { id: 'cleanup', title: '清理测试用户', description: '场景收尾，删除本次创建的数据。', calls: [{ id: 'delete', name: '删除测试用户', method: 'DELETE', path: '/api/system/users/{{userId}}', expect: '测试用户删除成功', response: { code: 0, data: true } }] },
  ] },
  { id: 'duplicate', module: '用户管理', title: '重复用户名被阻止', purpose: '已有用户名再次提交时，系统提示冲突且不产生重复数据。', actions: [
    { id: 'enter', title: '进入用户管理页', description: '获取现有用户名，作为本次重复输入。', calls: [listCall] },
    { id: 'submit', title: '提交重复用户名', description: '填写已存在的用户名并保存。', calls: [{ id: 'duplicate-user', name: '尝试创建重复用户', method: 'POST', path: '/api/system/users', body: { username: 'admin', name: '重复用户' }, expect: '返回用户名已存在，未创建新用户', response: { code: 'USER_ALREADY_EXISTS', message: '用户名已存在' } }] },
    { id: 'verify', title: '确认列表未变化', description: '检查用户总数和原用户信息保持一致。', calls: [{ ...listCall, id: 'verify-list', expect: '用户总数没有增加' }] },
  ] },
  { id: 'role', module: '角色管理', title: '为角色分配菜单权限', purpose: '管理员为普通角色分配菜单，重新查询确认授权结果。', actions: [
    { id: 'enter', title: '进入角色管理页', description: '加载角色列表。', calls: [{ id: 'role-list', name: '查询角色', method: 'GET', path: '/api/system/roles', expect: '包含普通角色', response: { code: 0, data: [{ id: 'role_user', name: '普通用户' }] } }] },
    { id: 'submit', title: '选择菜单并保存', description: '勾选用户管理菜单，提交授权。', calls: [{ id: 'grant', name: '保存菜单权限', method: 'POST', path: '/api/system/roles/role_user/menus', body: { menuIds: ['user-management'] }, expect: '授权成功', response: { code: 0, data: true } }] },
    { id: 'verify', title: '重新打开确认权限', description: '再次打开权限配置，核对已勾选菜单。', calls: [{ id: 'role-menu', name: '查询菜单权限', method: 'GET', path: '/api/system/roles/role_user/menus', expect: '包含刚分配的菜单', response: { code: 0, data: ['user-management'] } }] },
  ] },
]
