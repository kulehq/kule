# Runs in the interactive desktop session (through run-interactive.ps1).
# M2 listeners, M1 ConPTY, M4 tools, M8 process end, M9 reach, M3 proxy path.
$wm = "C:\kule-setup\wm.exe"; $ws = "C:\kule-setup\ws.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$ps = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
$node = "C:\Program Files\nodejs\node.exe"
$me = $env:USERNAME
New-Item -ItemType Directory -Force "$w\rw\sub" | Out-Null
"hello" | Set-Content "$w\rw\f.txt"
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA","--env","USERPROFILE=$env:USERPROFILE","--env","APPDATA=$env:APPDATA")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\$me","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 40 }
"session: $((Get-Process -Id $PID).SessionId); user: $me; elevated: $(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators'))"

T "0. whoami inside (user32 importer)" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe","/groups","/fo","list"))

"`n#### M2 listeners"
$server = "require('http').createServer((q,s)=>s.end('ok')).listen(18090,'127.0.0.1',()=>console.log('listening'))"
foreach ($net in @("deny","proxy-loopback:47700","allow")) {
  T "M2 node listener inside; host connects; net=$net" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--net",$net,"--cwd","$w\rw","--probe-port","18090","--after","3000","--timeout","15000","--",$node,"-e","`"$server`""))
}

"`n#### M1 ConPTY"
T "M1 conpty-host inside (ui disabled)" (@("--rw","$w\rw","--ro","C:\kule-setup") + $parents + $env + @("--cwd","$w\rw","--timeout","20000","--",$wm,"conpty-host","--",$cmd,"/d","/s","/c","echo conpty-works"))
T "M1b conpty-host inside (ui enabled)" (@("--rw","$w\rw","--ro","C:\kule-setup","--ui") + $parents + $env + @("--cwd","$w\rw","--timeout","20000","--",$wm,"conpty-host","--",$cmd,"/d","/s","/c","echo conpty-works"))

"`n#### M4 tools"
Remove-Item -Recurse -Force "$w\rw\repo" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\repo" | Out-Null
"hello" | Set-Content "$w\rw\repo\a.txt"
@"
@echo off
cd /d $w\rw\repo
"C:\Program Files\Git\cmd\git.exe" --version
"C:\Program Files\Git\cmd\git.exe" init -q . && echo [init ok] || echo [init FAIL]
"C:\Program Files\Git\cmd\git.exe" -c user.email=a@b -c user.name=t add . && "C:\Program Files\Git\cmd\git.exe" -c user.email=a@b -c user.name=t commit -qm one && echo [commit ok] || echo [commit FAIL]
"C:\Program Files\Git\cmd\git.exe" status --short && echo [status ok] || echo [status FAIL]
"C:\Program Files\Git\cmd\git.exe" log --oneline
"@ | Set-Content "$w\rw\g.cmd" -Encoding Ascii
T "M4 git (--ro Git, HOME in rw)" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $parents + $env + @("--env","HOME=$w\rw","--cwd","$w\rw\repo","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\g.cmd"))
T "M4 git with GIT_CEILING_DIRECTORIES" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $parents + $env + @("--env","HOME=$w\rw","--env","GIT_CEILING_DIRECTORIES=$w","--cwd","$w\rw\repo","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\g.cmd"))
New-Item -ItemType Directory -Force "$w\rw\proj","$w\rw\appdata" | Out-Null
'{"name":"p","version":"1.0.0","private":true}' | Set-Content "$w\rw\proj\package.json"
"console.log('cwd', process.cwd()); console.log('realpath', (()=>{try{return require('fs').realpathSync.native('.')}catch(e){return 'FAIL '+e.code}})()); console.log('readdir', require('fs').readdirSync('..').length)" | Set-Content "$w\rw\proj\i.js"
T "M4 node realpath + readdir" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--cwd","$w\rw\proj","--timeout","60000","--",$node,"i.js"))
T "M4 npm install left-pad (net allow)" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--env","APPDATA=$w\rw\appdata","--env","LOCALAPPDATA=$w\rw\appdata","--env","HOME=$w\rw","--net","allow","--cwd","$w\rw\proj","--timeout","180000","--",$cmd,"/d","/s","/c","npm --version && npm install --no-audit --no-fund --loglevel=error left-pad@1.3.0 && dir /b node_modules && echo [npm ok] || echo [npm FAIL]"))
T "M4 powershell inside" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$ps,"-NoProfile","-c","'ps-ran'; Get-Location | % Path; (Get-ChildItem).Count"))
T "M4 cd into read-only dir" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","cd /d ""C:\Program Files\Git\cmd"" && echo [cd ro ok] || echo [cd ro FAIL]"))

"`n#### M8 process end"
T "M8 detached child survives the foreground?" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--linger","ping.exe","--",$cmd,"/d","/s","/c","start /b ping -t 127.0.0.1 >nul & timeout /t 2 >nul & tasklist /fi ""imagename eq ping.exe"" /nh & echo started"))

"`n#### M9 reach from inside"
Add-Type -AssemblyName System.Security
$blob = [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes("secret"), $null, 'CurrentUser'))
"dpapi-probe" | Set-Content "$w\dpapi.txt"
@"
@echo off
echo [cmdkey] & cmdkey /list 2>&1 | find /c /v ""
echo [pipes] & dir \\.\pipe\\ 2>&1 | find /c /v ""
echo [dpapi] & $ps -NoProfile -c "Add-Type -AssemblyName System.Security; try { [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String('$blob'), `$null, 'CurrentUser')) } catch { 'refused: ' + `$_.Exception.InnerException.Message }"
echo [clipboard] & $ps -NoProfile -c "try { `$c = Get-Clipboard -Raw; 'READ ' + [string]`$c.Length } catch { 'refused: ' + `$_.Exception.Message }"
echo [integrity] & whoami /groups | findstr /i "mandatory"
echo [other user files] & type C:\Users\kulespike\mxcwork\rw\f.txt 2>&1
"@ | Set-Content "$w\rw\m9.cmd" -Encoding Ascii
T "M9 (ui disabled)" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\m9.cmd"))
T "M9b clipboard with ui enabled" (@("--rw","$w\rw","--ui") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$ps,"-NoProfile","-c","try { `$c = Get-Clipboard -Raw; 'READ ' + [string]`$c.Length } catch { 'refused: ' + `$_.Exception.Message }"))

"`n#### M3 proxy path"
$sid = (& $ws sid Kule.Proxy 2>&1 | Select-Object -Last 1)
"Kule.Proxy sid: $sid"
Get-Process -Name wm -ErrorAction SilentlyContinue | Stop-Process -Force
$sinkLog = "$w\sink.log"; Remove-Item $sinkLog -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $ws -ArgumentList @("run","--name","Kule.Proxy","--grant","C:\kule-setup","--cap","S-1-15-3-1","--cap","S-1-15-3-3","--","$wm","http-sink","127.0.0.1:47700") -RedirectStandardOutput $sinkLog -RedirectStandardError "$w\sink.err" -PassThru -WindowStyle Hidden
Start-Sleep 3
"sink (AppContainer) log: $(Get-Content $sinkLog -Raw -ErrorAction SilentlyContinue)"
"host connect to sink: $(& $wm connect 127.0.0.1:47700)"
T "M3a container proxy:47700:Kule.Proxy -> curl via proxy" (@("--rw","$w\rw") + $parents + $env + @("--net","proxy:47700:Kule.Proxy","--cwd","$w\rw","--timeout","20000","--","C:\Windows\System32\curl.exe","-s","-S","-m","8","-o","NUL","-w","http %{http_code}","http://example.com/"))
T "M3b container proxy-loopback:47700 (identity-less) -> curl via proxy" (@("--rw","$w\rw") + $parents + $env + @("--net","proxy-loopback:47700","--cwd","$w\rw","--timeout","20000","--","C:\Windows\System32\curl.exe","-s","-S","-m","8","-o","NUL","-w","http %{http_code}","http://example.com/"))
T "M3c container proxy:47700:Kule.Proxy -> direct connect bypassing proxy" (@("--rw","$w\rw") + $parents + $env + @("--net","proxy:47700:Kule.Proxy","--cwd","$w\rw","--timeout","20000","--","C:\Windows\System32\curl.exe","-s","-S","-m","8","--noproxy","*","-o","NUL","-w","http %{http_code}","http://93.184.215.14/"))
"sink log after: $(Get-Content $sinkLog -Raw -ErrorAction SilentlyContinue)"
"sink err: $(Get-Content "$w\sink.err" -Raw -ErrorAction SilentlyContinue)"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; Get-Process -Name wm -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
"`n-- sink outside any AppContainer"
$p2 = Start-Process -FilePath $wm -ArgumentList @("http-sink","127.0.0.1:47701") -RedirectStandardOutput "$w\sink2.log" -PassThru -WindowStyle Hidden
Start-Sleep 2
T "M3d plain sink, container proxy:47701:Kule.Proxy (peer named but not the listener)" (@("--rw","$w\rw") + $parents + $env + @("--net","proxy:47701:Kule.Proxy","--cwd","$w\rw","--timeout","20000","--","C:\Windows\System32\curl.exe","-s","-S","-m","8","-o","NUL","-w","http %{http_code}","http://example.com/"))
T "M3e plain sink, container proxy-loopback:47701" (@("--rw","$w\rw") + $parents + $env + @("--net","proxy-loopback:47701","--cwd","$w\rw","--timeout","20000","--","C:\Windows\System32\curl.exe","-s","-S","-m","8","-o","NUL","-w","http %{http_code}","http://example.com/"))
"sink2 log: $(Get-Content "$w\sink2.log" -Raw -ErrorAction SilentlyContinue)"
Stop-Process -Id $p2.Id -Force -ErrorAction SilentlyContinue
