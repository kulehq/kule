$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
$ws = 'C:\kule-setup\ws.exe'
$root = Join-Path $env:LOCALAPPDATA 'kule-spike'; $copy = "$root\copy"; $other = "$root\other-task"
New-Item -ItemType Directory -Force $copy, $other | Out-Null
Copy-Item $ws "$copy\ws.exe" -Force; Copy-Item C:\Windows\System32\curl.exe "$copy\c.exe" -Force; 'inside' | Set-Content "$copy\file.txt"; 'other' | Set-Content "$other\file.txt"
'SECRET' | Set-Content "$env:USERPROFILE\kule-secret.txt"; Remove-Item "$env:USERPROFILE\esc.txt" -ErrorAction SilentlyContinue
icacls "$copy\ws.exe" /setintegritylevel M | Out-Null
$listener = Start-Job -ScriptBlock { $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 18080); $l.Start(); while ($true) { $c = $l.AcceptTcpClient(); Start-Sleep -Milliseconds 100; $c.Close() } }
Start-Sleep 1
"logon sid: " + (& $ws logon-sid)
foreach ($level in 'low','limited','lockdown') {
  "=== restricted token: $level"
  & $ws run --restricted $level --grant $copy --cwd $copy -- "$copy\ws.exe" probe $copy 2>&1 | Where-Object { $_ -notmatch "Successfully processed|^\[spike\] restricted" }
  "  profile esc.txt exists: " + (Test-Path "$env:USERPROFILE\esc.txt"); Remove-Item "$env:USERPROFILE\esc.txt" -ErrorAction SilentlyContinue
}
"=== AppContainer, same probe (for the table):"
& $ws run --name kulespike1 --grant $copy --cwd $copy -- "$copy\ws.exe" probe $copy 2>&1 | Where-Object { $_ -match "profile|other task|Git|git|node|NUL" }
Stop-Job $listener; Remove-Job $listener -Force
