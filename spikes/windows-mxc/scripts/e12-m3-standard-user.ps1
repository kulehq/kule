# M3 (the proxy path) repeated as a STANDARD, interactive user. Run through run-interactive.ps1
# from the kulespike account, whose console session was made by autologon.
# The tools are copied into the user's own folder: granting a path to an AppContainer profile
# changes its ACL, and a standard user cannot do that in C:\kule-setup.
$src = "C:\kule-setup"
$w = "$env:USERPROFILE\mxcwork"
New-Item -ItemType Directory -Force "$w\rw\sub", "$w\bin" | Out-Null
Copy-Item "$src\wm.exe", "$src\ws.exe" "$w\bin" -Force
$wm = "$w\bin\wm.exe"; $ws = "$w\bin\ws.exe"
$me = $env:USERNAME
$curl = "C:\Windows\System32\curl.exe"
"hello" | Set-Content "$w\rw\f.txt"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
"session: $((Get-Process -Id $PID).SessionId); user: $me; elevated: $(([Security.Principal.WindowsPrincipal]$id).IsInRole('Administrators'))"
"integrity: $((whoami /groups | Select-String 'Mandatory Level') -join ' | ')"
"build: $((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild).$((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').UBR)"
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA","--env","USERPROFILE=$env:USERPROFILE","--env","APPDATA=$env:APPDATA")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\$me","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 40 }
function CurlArgs($net, $extra) { @("--rw","$w\rw","--ui") + $parents + $env + @("--net",$net,"--cwd","$w\rw","--timeout","20000","--",$curl,"-s","-S","-m","8") + $extra + @("-o","NUL","-w","http %{http_code}") }

T "0. whoami inside, ancestors enumerated, ui on" (@("--rw","$w\rw","--ui") + $parents + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe","/groups","/fo","list"))
T "0b. whoami inside, no ancestors" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe","/user"))

"`n#### M3 proxy path, standard user"
$sid = (& $ws sid Kule.Proxy 2>&1 | Select-Object -Last 1)
"Kule.Proxy sid: $sid"
Get-Process -Name wm -ErrorAction SilentlyContinue | Stop-Process -Force
$sinkLog = "$w\sink.log"; Remove-Item $sinkLog, "$w\sink.err" -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $ws -ArgumentList @("run","--name","Kule.Proxy","--grant","$w\bin","--cap","S-1-15-3-1","--cap","S-1-15-3-3","--","$wm","http-sink","127.0.0.1:47700") -RedirectStandardOutput $sinkLog -RedirectStandardError "$w\sink.err" -PassThru -WindowStyle Hidden
Start-Sleep 3
"sink (AppContainer) log: $(Get-Content $sinkLog -Raw -ErrorAction SilentlyContinue)"
"sink err: $(Get-Content "$w\sink.err" -Raw -ErrorAction SilentlyContinue)"
"host connect to sink: $(& $wm connect 127.0.0.1:47700 2>&1)"
T "M3a container proxy:47700:Kule.Proxy -> curl via proxy" (CurlArgs "proxy:47700:Kule.Proxy" @("http://example.com/"))
T "M3b container proxy-loopback:47700 (identity-less) -> curl via proxy" (CurlArgs "proxy-loopback:47700" @("http://example.com/"))
T "M3c container proxy:47700:Kule.Proxy -> direct connect bypassing proxy" (CurlArgs "proxy:47700:Kule.Proxy" @("--noproxy","*","http://93.184.215.14/"))
T "M3c2 container proxy:47700:Kule.Proxy -> direct connect to a name bypassing proxy" (CurlArgs "proxy:47700:Kule.Proxy" @("--noproxy","*","http://example.com/"))
T "M3a2 same as M3a, https" (CurlArgs "proxy:47700:Kule.Proxy" @("https://example.com/"))
"sink log after: $(Get-Content $sinkLog -Raw -ErrorAction SilentlyContinue)"
"sink err: $(Get-Content "$w\sink.err" -Raw -ErrorAction SilentlyContinue)"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; Get-Process -Name wm -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
"`n-- sink outside any AppContainer"
$p2 = Start-Process -FilePath $wm -ArgumentList @("http-sink","127.0.0.1:47701") -RedirectStandardOutput "$w\sink2.log" -PassThru -WindowStyle Hidden
Start-Sleep 2
T "M3d plain sink, container proxy:47701:Kule.Proxy (peer named but not the listener)" (CurlArgs "proxy:47701:Kule.Proxy" @("http://example.com/"))
T "M3e plain sink, container proxy-loopback:47701" (CurlArgs "proxy-loopback:47701" @("http://example.com/"))
"sink2 log: $(Get-Content "$w\sink2.log" -Raw -ErrorAction SilentlyContinue)"
Stop-Process -Id $p2.Id -Force -ErrorAction SilentlyContinue
"`n-- firewall and isolation state as seen by this user"
try { Get-NetFirewallProfile | Select Name,Enabled,DefaultInboundAction,DefaultOutboundAction | ft -auto | Out-String -Width 160 } catch { "profiles: $($_.Exception.Message)" }
try { CheckNetIsolation LoopbackExempt -s 2>&1 } catch { "loopback: $($_.Exception.Message)" }
try { (Get-NetFirewallRule | ? { $_.Package }).Count.ToString() + " package-scoped rules" } catch { "rules: $($_.Exception.Message)" }
"done"
