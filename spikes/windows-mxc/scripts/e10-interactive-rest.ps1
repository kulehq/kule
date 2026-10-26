$wm = "C:\kule-setup\wm.exe"; $ws = "C:\kule-setup\ws.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$ps = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
$me = $env:USERNAME
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA","--env","USERPROFILE=$env:USERPROFILE","--env","APPDATA=$env:APPDATA","--ui")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\$me","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 40 }
"#### M1 ConPTY"
T "M1 conpty-host, cmd echo + sleep" (@("--rw","$w\rw","--ro","C:\kule-setup") + $parents + $env + @("--cwd","$w\rw","--timeout","20000","--",$wm,"conpty-host","--",$cmd,"/d","/s","/c","echo conpty-works & ping -n 2 127.0.0.1 >nul & echo done"))
T "M1c conpty-host, powershell one-liner" (@("--rw","$w\rw","--ro","C:\kule-setup") + $parents + $env + @("--cwd","$w\rw","--timeout","30000","--",$wm,"conpty-host","--",$ps,"-NoProfile","-c","'pwsh-under-conpty'"))
"`n#### M8"
"@echo off`r`nstart /b ping -t 127.0.0.1 >nul`r`ntimeout /t 2 >nul`r`ntasklist /fi ""imagename eq ping.exe"" /nh`r`necho started" | Set-Content "$w\rw\m8.cmd" -Encoding Ascii
T "M8 detached child: running during, gone after?" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--linger","ping.exe","--",$cmd,"/d","/s","/c","$w\rw\m8.cmd"))
"`n#### M4 git with safe.directory"
Remove-Item -Recurse -Force "$w\rw\repo" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\repo" | Out-Null
"hello" | Set-Content "$w\rw\repo\a.txt"
T "M4 git init/commit/status/log" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $parents + $env + @("--env","HOME=$w\rw","--env","GIT_CONFIG_COUNT=1","--env","GIT_CONFIG_KEY_0=safe.directory","--env","GIT_CONFIG_VALUE_0=*","--cwd","$w\rw\repo","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\g.cmd"))
"`n#### M9 credentials and pipes"
cmdkey /add:kuletest.example /user:kuleuser /pass:kulepass | Out-Null
$pipeLog = "$w\pipe.log"
$pp = Start-Process -FilePath $ws -ArgumentList @("pipe-server","kuletest") -RedirectStandardOutput $pipeLog -PassThru -WindowStyle Hidden
Start-Sleep 2
"pipe server on host: $(Get-Content $pipeLog -Raw -ErrorAction SilentlyContinue)"
"host client: $(& $ws pipe-client kuletest 2>&1 | Select-Object -Last 2)"
@"
@echo off
echo [cmdkey] & cmdkey /list 2>&1 | findstr /i "kuletest NONE"
echo [pipe owner-only] & $ps -NoProfile -c "try { `$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', 'kuletest', 'InOut'); `$p.Connect(2000); 'CONNECTED' } catch { 'refused: ' + `$_.Exception.Message }"
echo [pipe list has it] & dir \\.\pipe\\ | findstr /i kuletest
"@ | Set-Content "$w\rw\m9b.cmd" -Encoding Ascii
T "M9 stored credential + owner-only pipe" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--timeout","60000","--",$cmd,"/d","/s","/c","$w\rw\m9b.cmd"))
cmdkey /delete:kuletest.example | Out-Null
Stop-Process -Id $pp.Id -Force -ErrorAction SilentlyContinue
"`n#### M5 again, with ancestors enumerable (ui on)"
Remove-Item -Recurse -Force "$w\rw\ro","$w\rw\deny2","$w\outside","$w\rw\jlink" -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\ro\deny","$w\rw\deny2","$w\outside" | Out-Null
"ro-file" | Set-Content "$w\rw\ro\f.txt"; "deny-file" | Set-Content "$w\rw\ro\deny\f.txt"; "deny2-file" | Set-Content "$w\rw\deny2\f.txt"; "outside-file" | Set-Content "$w\outside\f.txt"
cmd /c mklink /J "$w\rw\jlink" "$w\outside" | Out-Null
@"
@echo off
echo [write rw] & (echo x> $w\rw\new.txt && echo ok) || echo FAIL
echo [write inside ro] & (echo x> $w\rw\ro\new.txt && echo WROTE) || echo refused
echo [read ro] & type $w\rw\ro\f.txt || echo FAIL
echo [read denied inside ro] & (type $w\rw\ro\deny\f.txt && echo READ) || echo refused
echo [write denied inside rw] & (echo x> $w\rw\deny2\new.txt && echo WROTE) || echo refused
echo [read denied inside rw] & (type $w\rw\deny2\f.txt && echo READ) || echo refused
echo [list rw: is deny2 visible] & dir /b $w\rw | findstr deny2
echo [read via junction to ungranted] & (type $w\rw\jlink\f.txt && echo READ) || echo refused
echo [read ungranted directly] & (type $w\outside\f.txt && echo READ) || echo refused
echo [enumerate-only parent: list] & (dir /b $w && echo LISTED) || echo refused
echo [enumerate-only parent: read file] & (type $w\dpapi.txt && echo READ) || echo refused
echo [create dir in rw] & (mkdir $w\rw\made && echo ok) || echo FAIL
echo [rename in rw] & (ren $w\rw\new.txt new2.txt && echo ok) || echo FAIL
echo [delete in rw] & (del $w\rw\new2.txt && echo ok) || echo FAIL
echo [symlink in rw] & (mklink $w\rw\sl.txt $w\rw\f.txt && echo MADE) || echo refused
echo [junction in rw to outside] & (mklink /J $w\rw\j2 $w\outside && echo MADE) || echo refused
echo [read through new junction] & (type $w\rw\j2\f.txt && echo READ) || echo refused
"@ | Set-Content "$w\rw\m5.cmd" -Encoding Ascii
T "M5 precedence" (@("--rw","$w\rw","--ro","$w\rw\ro","--deny","$w\rw\ro\deny","--deny","$w\rw\deny2") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\m5.cmd"))
Remove-Item "$w\rw\j2" -Force -ErrorAction SilentlyContinue
