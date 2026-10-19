param([string]$BaseUrl = 'http://127.0.0.1:31080', [switch]$SkipArthas)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $repo '.devflow/acceptance/Java 中文 空格'
New-Item -ItemType Directory -Force (Join-Path $fixture 'src/main/java/example'), (Join-Path $fixture 'src/main/resources') | Out-Null
@'
<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion><parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId><version>3.5.14</version></parent><groupId>devflow.acceptance</groupId><artifactId>java-acceptance</artifactId><version>1</version><properties><java.version>17</java.version><start-class>example.Application</start-class></properties><dependencies><dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId></dependency></dependencies></project>
'@ | Set-Content -Encoding utf8 (Join-Path $fixture 'pom.xml')
@'
package example;
@org.springframework.boot.autoconfigure.SpringBootApplication
public class Application {
  public static void main(String[] args) throws Exception {
    var application = new org.springframework.boot.SpringApplication(Application.class);
    application.setWebApplicationType(org.springframework.boot.WebApplicationType.NONE);
    var context = application.run(args);
    var environment = context.getEnvironment();
    System.out.println("READY|" + String.join(",", environment.getActiveProfiles()) + "|" + environment.getProperty("acceptance.marker") + "|" + System.getProperty("acceptance.jvm") + "|" + environment.getProperty("acceptance.name"));
    System.err.println("STDERR|中文错误输出");
    var child = new ProcessBuilder(System.getProperty("java.home") + "/bin/java", "-cp", System.getProperty("java.class.path"), "example.Child").inheritIO().start();
    System.out.println("CHILD|" + child.pid());
    while (true) { System.out.println("TICK|" + System.currentTimeMillis()); Thread.sleep(1000); }
  }
}
'@ | Set-Content -Encoding utf8 (Join-Path $fixture 'src/main/java/example/Application.java')
@'
package example;
public class Child { public static void main(String[] args) throws Exception { Thread.sleep(600000); } }
'@ | Set-Content -Encoding utf8 (Join-Path $fixture 'src/main/java/example/Child.java')
'acceptance.marker=qa-value' | Set-Content -Encoding utf8 (Join-Path $fixture 'src/main/resources/application-qa.properties')
'acceptance.marker=dev-value' | Set-Content -Encoding utf8 (Join-Path $fixture 'src/main/resources/application-dev.properties')
$token = (Invoke-RestMethod "$BaseUrl/api/session" -Headers @{'X-DevFlow-Client'='web'}).token
$headers = @{Authorization="Bearer $token"}
function Api($path, $body = $null) {
  if ($null -eq $body) { return Invoke-RestMethod "$BaseUrl$path" -Headers $headers }
  Invoke-RestMethod "$BaseUrl$path" -Headers $headers -Method Post -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 20 -Compress)))
}
function Mcp($name, $arguments) { Api '/mcp' @{jsonrpc='2.0';id=1;method='tools/call';params=@{name=$name;arguments=$arguments}} }
$checks = [Collections.Generic.List[string]]::new()
function Check($condition, $message) { if (!$condition) { throw "FAILED: $message" }; $checks.Add($message); Write-Output "PASS: $message" }
function Output { Api ("/api/runtime/java/output?projectPath=" + [uri]::EscapeDataString($fixture)) }
function WaitOutput($predicate, [int]$seconds = 120) {
  $deadline = [datetime]::UtcNow.AddSeconds($seconds)
  do { $value = Output; if (& $predicate $value) { return $value }; if ($value.state -eq 'failed') { $value | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 (Join-Path $fixture 'failure.json'); throw ('Java failed: ' + (($value.lines | Select-Object -Last 12).text -join "`n")) }; Start-Sleep -Milliseconds 500 } while ([datetime]::UtcNow -lt $deadline)
  throw ('Timed out: ' + (($value.lines | Select-Object -Last 12).text -join "`n"))
}
$before = Api '/api/workspace'
if ($before.dirty) { throw 'Save the current workspace before running acceptance.' }
$current = $null
try {
  $current = Api '/api/workspace' @{expectedRevision=$before.revision;operation=@{type='open';path=$fixture}}
  Check ($current.projects.Count -eq 1) '中文及空格路径内的真实 Spring Boot 服务发现'
  $options = Api ("/api/runtime/java/options?projectPath=" + [uri]::EscapeDataString($fixture))
  Check ($options.profiles -contains 'qa' -and $options.profiles -contains 'dev') '识别 Spring Profile 配置文件'
  $start = @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='start';mainClass='example.Application';jvmArgs=@('-Xmx256m','-Dacceptance.jvm=中文 空格');args=@('--acceptance.name=普通 空格');profiles=@('qa')}}
  $started = Api '/api/runtime/java' $start
  Check ($started.state -eq 'starting') '异步启动返回构建中状态'
  $duplicate = Mcp 'java_execute' $start
  Check ($duplicate.result.isError) 'Web 与 MCP 共用状态并拒绝重复启动'
  $running = WaitOutput { param($value) ($value.lines.text -join "`n") -match 'READY\|qa\|qa-value\|中文 空格\|普通 空格' }
  $running | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 (Join-Path $fixture 'running.json')
  Check ($running.state -eq 'running') 'JVM 参数、普通程序参数、Spring Profile 均真实生效'
  $withChild = WaitOutput { param($value) ($value.lines.text -join "`n") -match 'CHILD\|\d+' }
  $childLine = $withChild.lines.text | Where-Object { $_ -match '^CHILD\|' } | Select-Object -First 1
  $childPid = [int]($childLine.Split('|')[1])
  Check (($withChild.lines | Where-Object stream -eq stderr).text -match 'STDERR') 'stderr 与 stdout 均捕获'
  $metricsDeadline = [datetime]::UtcNow.AddSeconds(8)
  do {
    $runtime = (Api '/api/runtime/projects').projects | Select-Object -First 1
    if ($runtime.javaPids -contains $running.pid -and $runtime.javaPids -contains $childPid) { break }
    Start-Sleep -Milliseconds 500
  } while ([datetime]::UtcNow -lt $metricsDeadline)
  Check ($runtime.javaPids.Count -eq 2 -and $runtime.javaPids -contains $running.pid -and $runtime.javaPids -contains $childPid) '可操作 PID 仅包含 Java 应用及 Java 子进程，排除 conhost'
  $cursorResult = Api ("/api/runtime/java/output?projectPath=" + [uri]::EscapeDataString($fixture) + '&after=' + $running.nextCursor + '&runId=' + $running.runId)
  Check (@($cursorResult.lines | Where-Object sequence -le $running.nextCursor).Count -eq 0) '增量日志游标不重复返回旧日志'
  $foreign = Mcp 'java_execute' @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='stop';pid=$PID}}
  Check ($foreign.result.isError) '拒绝终止不属于项目的 PID'
  $stale = Mcp 'java_execute' @{projectPath=$fixture;expectedRevision=0;operation=@{type='stop'}}
  Check ($stale.result.isError) '拒绝过期工作区版本的进程操作'
  if (!$SkipArthas) {
    $attach = Mcp 'java_execute' @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='attach';pid=$running.pid}}
    Check (!$attach.result.isError) '通过 MCP 请求官方 Arthas 附加'
    $attached = WaitOutput { param($value) !$value.attaching } 120
    $attached | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 (Join-Path $fixture 'arthas.json')
    Check (!!$attached.arthasUrl) 'Arthas 附加成功并返回 loopback 地址'
    $arthas = Invoke-WebRequest $attached.arthasUrl -TimeoutSec 10
    Check ($arthas.StatusCode -eq 200) 'Arthas HTTP 控制台可访问'
  }
  $stop = Mcp 'java_execute' @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='stop'}}
  Check (!$stop.result.isError) '通过 MCP 停止 Web 启动的 Java 服务'
  Start-Sleep -Seconds 2
  Check (!(Get-Process -Id $running.pid -ErrorAction SilentlyContinue) -and !(Get-Process -Id $childPid -ErrorAction SilentlyContinue)) '停止清理父进程及真实 Java 子进程'
  $restart = Api '/api/runtime/java' $start
  Check ($restart.runId -ne $running.runId) '重新启动创建独立输出会话'
  $stopBuild = Api '/api/runtime/java' @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='stop'}}
  Check ($stopBuild.state -eq 'stopped') '构建阶段可取消启动'
  Start-Sleep -Seconds 1
  $checks | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $fixture 'checks.json')
} finally {
  if ($current) {
    try { Api '/api/runtime/java' @{projectPath=$fixture;expectedRevision=$current.revision;operation=@{type='stop'}} | Out-Null } catch { }
    $now = Api '/api/workspace'
    $path = if ($before.workspaceFile) { $before.workspaceFile } else { $before.workspaceRoot }
    if ($path) { Api '/api/workspace' @{expectedRevision=$now.revision;operation=@{type='open';path=$path}} | Out-Null }
    else { Api '/api/workspace' @{expectedRevision=$now.revision;operation=@{type='close'}} | Out-Null }
  }
}
