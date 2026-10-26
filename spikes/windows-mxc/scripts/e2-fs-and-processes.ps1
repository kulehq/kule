# M5 filesystem precedence, M8 process end, M9 reach from inside.
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$ps = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
Remove-Item -Recurse -Force "$w\rw","$w\outside" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\ro\deny","$w\rw\deny2","$w\outside","$w\rw\sub" | Out-Null
"rw-file" | Set-Content "$w\rw\f.txt"; "ro-file" | Set-Content "$w\rw\ro\f.txt"; "deny-file" | Set-Content "$w\rw\ro\deny\f.txt"
"deny2-file" | Set-Content "$w\rw\deny2\f.txt"; "outside-file" | Set-Content "$w\outside\f.txt"
cmd /c mklink /J "$w\rw\jlink" "$w\outside" | Out-Null
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
$script = @"
echo [write rw] & echo x> $w\rw\new.txt && echo ok || echo FAIL
echo [write inside ro] & echo x> $w\rw\ro\new.txt && echo WROTE || echo refused
echo [read ro] & type $w\rw\ro\f.txt || echo FAIL
echo [read denied inside ro] & type $w\rw\ro\deny\f.txt && echo READ || echo refused
echo [write denied inside rw] & echo x> $w\rw\deny2\new.txt && echo WROTE || echo refused
echo [read denied inside rw] & type $w\rw\deny2\f.txt && echo READ || echo refused
echo [read via junction to ungranted] & type $w\rw\jlink\f.txt && echo READ || echo refused
echo [list via junction] & dir /b $w\rw\jlink && echo LISTED || echo refused
echo [read ungranted directly] & type $w\outside\f.txt && echo READ || echo refused
echo [list parent of rw] & dir /b $w && echo LISTED || echo refused
echo [list rw] & dir /b $w\rw || echo FAIL
echo [cwd] & cd
echo [read userprofile] & dir /b %USERPROFILE% && echo LISTED || echo refused
echo [read C:\] & dir /b C:\ && echo LISTED || echo refused
echo [read kule-setup] & dir /b C:\kule-setup && echo LISTED || echo refused
"@
$script | Set-Content "$w\rw\t.cmd" -Encoding Ascii
"== M5 precedence (rw=$w\rw, ro=$w\rw\ro, deny=$w\rw\ro\deny + $w\rw\deny2)"
& $wm run --rw "$w\rw" --ro "$w\rw\ro" --deny "$w\rw\ro\deny" --deny "$w\rw\deny2" @env --cwd "$w\rw\sub" -- $cmd /d /s /c "$w\rw\t.cmd"
"`n== M5b junction target granted read-only separately"
& $wm run --rw "$w\rw" --ro "$w\outside" @env --cwd "$w\rw" -- $cmd /d /s /c "type $w\rw\jlink\f.txt && echo READ-via-junction || echo refused"
"`n== M8 process end: start /b ping -t, then exit"
& $wm run --rw "$w\rw" @env --cwd "$w\rw" --linger ping.exe -- $cmd /d /s /c "start /b ping -t 127.0.0.1 >nul & timeout /t 1 >nul & echo started"
"`n== M9 reach from inside"
$blob = [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes("secret"), $null, 'CurrentUser'))
$m9 = @"
echo [cmdkey] & cmdkey /list && echo LISTED || echo refused
echo [pipes] & dir \\.\pipe\\ | find /c /v "" 
echo [clipboard] & $ps -NoProfile -c "try { Get-Clipboard | Out-Null; 'READ' } catch { 'refused: ' + `$_.Exception.Message }"
echo [dpapi] & $ps -NoProfile -c "try { [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String('$blob'), `$null, 'CurrentUser')) } catch { 'refused: ' + `$_.Exception.Message }"
echo [whoami] & whoami /groups | findstr /i "package\|S-1-15" | find /c /v ""
echo [integrity] & whoami /groups | findstr /i "mandatory"
"@
$m9 | Set-Content "$w\rw\m9.cmd" -Encoding Ascii
& $wm run --rw "$w\rw" @env --cwd "$w\rw" -- $cmd /d /s /c "$w\rw\m9.cmd"
"`n== M9b same with UI enabled"
& $wm run --rw "$w\rw" @env --ui --cwd "$w\rw" -- $cmd /d /s /c "$ps -NoProfile -c ""try { Get-Clipboard | Out-Null; 'clipboard READ' } catch { 'clipboard refused: ' + `$_.Exception.Message }"""
