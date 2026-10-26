$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$ps = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
$me = $env:USERNAME
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA","--env","USERPROFILE=$env:USERPROFILE","--env","APPDATA=$env:APPDATA","--ui")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\$me","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 60 }
"#### M8 with a detached child started by PowerShell"
T "M8 detached child: running during, gone after?" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--linger","ping.exe","--timeout","30000","--",$ps,"-NoProfile","-c","Start-Process ping -ArgumentList '-t 127.0.0.1' -WindowStyle Hidden; Start-Sleep 2; 'ping processes inside: ' + (Get-Process ping -ErrorAction SilentlyContinue).Count; 'exiting'"))
"`n#### M5 tail: junctions made inside"
Remove-Item "$w\rw\j2" -Force -Recurse -ErrorAction SilentlyContinue
@"
@echo off
echo [junction in rw to outside] & (mklink /J $w\rw\j2 $w\outside && echo MADE) || echo refused
echo [read through new junction] & (type $w\rw\j2\f.txt && echo READ) || echo refused
echo [junction in rw to rw\ro] & (mklink /J $w\rw\j3 $w\rw\ro && echo MADE) || echo refused
echo [write through j3 into ro] & (echo x> $w\rw\j3\viaj.txt && echo WROTE) || echo refused
echo [read denied through j3] & (type $w\rw\j3\deny\f.txt && echo READ) || echo refused
echo [hardlink to ro file] & (mklink /H $w\rw\hl.txt $w\rw\ro\f.txt && echo MADE) || echo refused
echo [write via hardlink] & (echo x> $w\rw\hl.txt && echo WROTE) || echo refused
echo [8.3 short name of denied] & dir /x $w\rw | findstr /i deny2
echo [read denied by short name] & (type $w\rw\DENY2~1\f.txt && echo READ) || echo refused
echo [UNC loopback path] & (type \\127.0.0.1\C$\Users\$me\mxcwork\outside\f.txt && echo READ) || echo refused
echo [device path] & (type \\?\C:\Users\$me\mxcwork\outside\f.txt && echo READ) || echo refused
echo [GLOBALROOT path] & (type \\?\GLOBALROOT\Device\HarddiskVolume3\Users\$me\mxcwork\outside\f.txt && echo READ) || echo refused
"@ | Set-Content "$w\rw\m5b.cmd" -Encoding Ascii
T "M5 tail" (@("--rw","$w\rw","--ro","$w\rw\ro","--deny","$w\rw\ro\deny","--deny","$w\rw\deny2") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\m5b.cmd"))
Remove-Item "$w\rw\j2","$w\rw\j3" -Force -Recurse -ErrorAction SilentlyContinue
Remove-Item "$w\rw\hl.txt","$w\rw\ro\viaj.txt" -Force -ErrorAction SilentlyContinue
"`n#### M2b: a second container reaches the first container's listener?"
$node = "C:\Program Files\nodejs\node.exe"
$srv = Start-Process -FilePath $wm -ArgumentList (@("run","--rw","$w\rw","--ro","C:\Program Files\nodejs") + $parents + $env + @("--net","allow","--cwd","$w\rw","--timeout","25000","--",$node,"-e","`"require('http').createServer((q,s)=>s.end('ok')).listen(18092,'127.0.0.1',()=>console.log('listening'))`"")) -RedirectStandardOutput "$w\srv.log" -PassThru -WindowStyle Hidden
Start-Sleep 4
"server container log: $(Get-Content "$w\srv.log" -Raw -ErrorAction SilentlyContinue)"
T "M2b curl from another container (net allow)" (@("--rw","$w\rw") + $parents + $env + @("--net","allow","--cwd","$w\rw","--timeout","15000","--","C:\Windows\System32\curl.exe","-s","-m","5","http://127.0.0.1:18092/"))
"host curl: $(curl.exe -s -m 5 http://127.0.0.1:18092/ 2>&1)"
Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue
