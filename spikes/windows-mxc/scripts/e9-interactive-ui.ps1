# Interactive session, ui.disable=false (needed by every user32-importing tool).
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$ps = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
$node = "C:\Program Files\nodejs\node.exe"
$me = $env:USERNAME
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA","--env","USERPROFILE=$env:USERPROFILE","--env","APPDATA=$env:APPDATA","--ui")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\$me","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 40 }
"session: $((Get-Process -Id $PID).SessionId); user: $me"
T "0. whoami inside" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe","/groups","/fo","list"))
"`n#### M2 listeners"
$server = "require('http').createServer((q,s)=>s.end('ok')).listen(18090,'127.0.0.1',()=>console.log('listening'))"
foreach ($net in @("deny","proxy-loopback:47700","allow")) {
  T "M2 node listener inside; host connects; net=$net" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--net",$net,"--cwd","$w\rw","--probe-port","18090","--after","3000","--timeout","15000","--",$node,"-e","`"$server`""))
}
T "M2d listener on 0.0.0.0 (all addresses), net=deny" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--net","deny","--cwd","$w\rw","--probe-port","18091","--after","3000","--timeout","15000","--",$node,"-e","`"require('http').createServer((q,s)=>s.end('ok')).listen(18091,'0.0.0.0',()=>console.log('listening all'))`""))
"`n#### M1 ConPTY (longer-lived shell)"
T "M1 conpty-host, cmd echo + sleep" (@("--rw","$w\rw","--ro","C:\kule-setup") + $parents + $env + @("--cwd","$w\rw","--timeout","20000","--",$wm,"conpty-host","--",$cmd,"/d","/s","/c","echo conpty-works & ping -n 2 127.0.0.1 >nul & echo done"))
"`n#### M4 tools"
Remove-Item -Recurse -Force "$w\rw\repo","$w\rw\proj\node_modules" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\repo" | Out-Null
"hello" | Set-Content "$w\rw\repo\a.txt"
T "M4 git (--ro Git, HOME in rw)" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $parents + $env + @("--env","HOME=$w\rw","--cwd","$w\rw\repo","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\g.cmd"))
T "M4 node realpath + readdir" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--cwd","$w\rw\proj","--timeout","60000","--",$node,"i.js"))
T "M4 npm install left-pad (net allow)" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--env","APPDATA=$w\rw\appdata","--env","HOME=$w\rw","--net","allow","--cwd","$w\rw\proj","--timeout","180000","--",$cmd,"/d","/s","/c","npm --version && npm install --no-audit --no-fund --loglevel=error left-pad@1.3.0 && dir /b node_modules && echo [npm ok] || echo [npm FAIL]"))
T "M4 powershell inside" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$ps,"-NoProfile","-c","'ps-ran'; Get-Location | % Path; (Get-ChildItem).Count"))
T "M4 cargo --version (--ro .cargo .rustup)" (@("--rw","$w\rw","--ro","$env:USERPROFILE\.cargo","--ro","$env:USERPROFILE\.rustup") + $parents + $env + @("--env","CARGO_HOME=$env:USERPROFILE\.cargo","--env","RUSTUP_HOME=$env:USERPROFILE\.rustup","--cwd","$w\rw","--timeout","60000","--",$cmd,"/d","/s","/c","cargo --version && rustc --version"))
"`n#### M8"
T "M8 detached child: running during, gone after?" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--linger","ping.exe","--",$cmd,"/d","/s","/c","start /b ping -t 127.0.0.1 >nul & timeout /t 2 >nul & tasklist /fi ""imagename eq ping.exe"" /nh & echo started"))
"`n#### M9"
Add-Type -AssemblyName System.Security
$blob = [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes("secret"), $null, 'CurrentUser'))
@"
@echo off
echo [cmdkey] & cmdkey /list 2>&1
echo [dpapi] & $ps -NoProfile -c "Add-Type -AssemblyName System.Security; try { [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String('$blob'), `$null, 'CurrentUser')) } catch { 'refused: ' + `$_.Exception.InnerException.Message }"
echo [integrity] & whoami /groups | findstr /i "mandatory"
echo [appcontainer groups] & whoami /groups | findstr /i "S-1-15" | find /c /v ""
echo [pipe connect: ssh-agent] & $ps -NoProfile -c "try { `$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', 'openssh-ssh-agent', 'InOut'); `$p.Connect(1000); 'CONNECTED' } catch { 'refused: ' + `$_.Exception.Message }"
echo [registry HKCU] & reg query HKCU\Environment 2>&1
echo [registry HKCU write] & reg add HKCU\KuleSpikeProbe /f 2>&1
"@ | Set-Content "$w\rw\m9.cmd" -Encoding Ascii
T "M9 reach from inside (ui enabled)" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\m9.cmd"))
reg delete HKCU\KuleSpikeProbe /f 2>&1 | Out-Null
