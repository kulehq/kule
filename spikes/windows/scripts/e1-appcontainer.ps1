# E1/E2: what an AppContainer holds for files, tools and the network, from the person's own session.
$ErrorActionPreference = 'Continue'
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
$spike = 'C:\src\winspike\target\debug\winspike.exe'
$root = Join-Path $env:LOCALAPPDATA 'kule-spike'; $copy = "$root\copy"; $outside = "$root\outside"
Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $copy, $outside | Out-Null
'inside' | Set-Content "$copy\file.txt"; 'outside' | Set-Content "$outside\file.txt"
Copy-Item $spike "$copy\ws.exe"
$secret = "$env:USERPROFILE\kule-secret.txt"; 'SECRET' | Set-Content $secret
$name = 'kulespike1'
"runner: " + (& $spike whoami)
& $spike run --name $name --grant $copy -- cmd /c exit 0 2>$null; $sid = & $spike sid $name
"container sid: $sid"
$listener = Start-Job -ScriptBlock {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 18080); $l.Start()
  while ($true) { $c = $l.AcceptTcpClient(); Start-Sleep -Milliseconds 100; $c.Close() }
}
Start-Sleep 2
"host sees proxy: " + (& "$copy\ws.exe" connect 127.0.0.1 18080)

function Scenario($label, $cmd, $extra = @()) {
  Remove-Item "$copy\out.txt" -ErrorAction SilentlyContinue
  $p = Start-Process -FilePath $spike -ArgumentList (@('run','--name',$name,'--grant',$copy,'--cwd',$copy) + $extra + @('--','cmd','/c',"`"($cmd) > $copy\out.txt 2>&1`"")) -Wait -PassThru -NoNewWindow -RedirectStandardError "$root\err.txt"
  [string]$out = if (Test-Path "$copy\out.txt") { [string](Get-Content "$copy\out.txt" -Raw -ErrorAction SilentlyContinue) } else { '(no output file)' }
  $out = ($out -replace '\s+', ' ').Trim(); if ($out.Length -gt 100) { $out = $out.Substring(0,100) + '…' }
  "{0,-36} exit={1,-12} {2}" -f $label, $p.ExitCode, $out
}
"=== files"
Scenario 'identity'                 "$copy\ws.exe whoami"
Scenario 'read copy'                "type $copy\file.txt"
Scenario 'write copy'               "echo hi > $copy\w.txt && type $copy\w.txt"
Scenario 'read outside (ungranted)' "type $outside\file.txt"
Scenario 'write outside'            "echo hi > $outside\w.txt"
"  outside\w.txt exists: " + (Test-Path "$outside\w.txt")
Scenario 'delete outside file'      "del $outside\file.txt"
"  outside\file.txt still there: " + (Test-Path "$outside\file.txt")
Scenario 'read profile secret'      "type $secret"
Scenario 'list profile'             "dir /b $env:USERPROFILE"
Scenario 'read C:\src\kule'         'type C:\src\kule\Cargo.toml'
Scenario 'read system hosts file'   'type C:\Windows\System32\drivers\etc\hosts'
Scenario 'list Program Files\Git'   'dir /b C:\PROGRA~1\Git\cmd'
Scenario 'temp dir'                 'echo %TEMP% && echo t > %TEMP%\kule-spike.txt && dir /b %TEMP%'
Scenario 'env HOME/USERPROFILE'     'echo %USERPROFILE% %HOME% %APPDATA%'
Scenario 'registry HKCU read'       'reg query HKCU\Environment'
Scenario 'registry HKCU write'      'reg add HKCU\Software\KuleSpike /f'
Scenario 'NUL device read'          'type NUL && echo nul-read-ok'
Scenario 'NUL device write'         'echo x > NUL && echo nul-write-ok'
Scenario 'run copied exe (ws.exe)'  "$copy\ws.exe sleep 0 && echo ran"
Scenario 'run System32 curl'        'curl.exe --version'
"=== tools"
Scenario 'git version'              'git --version'
Scenario 'git init+commit'          "git init -q $copy\repo && cd /d $copy\repo && echo a>a && git add a && git -c user.name=s -c user.email=s@s commit -q -m a && git log --oneline"
Scenario 'node'                     'node -e console.log(process.version)'
Scenario 'npm version'              'npm --version'
Scenario 'powershell'               'powershell -NoProfile -c 1+1'
Scenario 'cargo (in profile)'       'cargo --version'
"=== network, no capability"
Scenario 'loopback proxy'           "$copy\ws.exe connect 127.0.0.1 18080"
Scenario 'internet 1.1.1.1:443'     "$copy\ws.exe connect 1.1.1.1 443"
Scenario 'listen 18081'             "$copy\ws.exe listen 18081"
"=== network, internetClient capability (S-1-15-3-1)"
Scenario 'loopback proxy'           "$copy\ws.exe connect 127.0.0.1 18080" @('--cap','S-1-15-3-1')
Scenario 'internet 1.1.1.1:443'     "$copy\ws.exe connect 1.1.1.1 443" @('--cap','S-1-15-3-1')
"=== network, privateNetworkClientServer (S-1-15-3-3)"
Scenario 'loopback proxy'           "$copy\ws.exe connect 127.0.0.1 18080" @('--cap','S-1-15-3-3')
Scenario 'gateway 192.168.64.1:22'  "$copy\ws.exe connect 192.168.64.1 22" @('--cap','S-1-15-3-3')
Scenario 'listen 18081'             "$copy\ws.exe listen 18081" @('--cap','S-1-15-3-3')
"=== network, loopback exemption, no capability (needs admin?)"
$ex = CheckNetIsolation.exe LoopbackExempt -a "-p=$sid" 2>&1; "  CheckNetIsolation add: exit=$LASTEXITCODE $($ex | Select-Object -Last 1)"
Scenario 'loopback proxy'           "$copy\ws.exe connect 127.0.0.1 18080"
Scenario 'loopback other port 22'   "$copy\ws.exe connect 127.0.0.1 22"
Scenario 'internet 1.1.1.1:443'     "$copy\ws.exe connect 1.1.1.1 443"
Scenario 'listen 18081'             "$copy\ws.exe listen 18081"
CheckNetIsolation.exe LoopbackExempt -d "-p=$sid" 2>&1 | Select-Object -Last 1
"=== job object"
Scenario 'no job: start detached sleeper' "start /b $copy\ws.exe sleep 25 & echo started"
Start-Sleep 1; "  sleeper alive after exit (no job): " + ((Get-Process ws -ErrorAction SilentlyContinue | Measure-Object).Count); Get-Process ws -ErrorAction SilentlyContinue | Stop-Process -Force
Scenario 'job: start detached sleeper'    "start /b $copy\ws.exe sleep 25 & echo started" @('--job')
Start-Sleep 1; "  sleeper alive after exit (job):    " + ((Get-Process ws -ErrorAction SilentlyContinue | Measure-Object).Count); Get-Process ws -ErrorAction SilentlyContinue | Stop-Process -Force
Stop-Job $listener; Remove-Job $listener -Force
Remove-Item $secret -Force
"done"
