$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
$spike = 'C:\src\winspike\target\debug\winspike.exe'
$root = Join-Path $env:LOCALAPPDATA 'kule-spike'; $copy = "$root\copy"
New-Item -ItemType Directory -Force $copy | Out-Null
Copy-Item $spike "$copy\ws.exe" -Force; Copy-Item C:\Windows\System32\curl.exe "$copy\c.exe" -Force; 'inside' | Set-Content "$copy\file.txt"
$listener = Start-Job -ScriptBlock { $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 18080); $l.Start(); while ($true) { $c = $l.AcceptTcpClient(); Start-Sleep -Milliseconds 100; $c.Close() } }
Start-Sleep 1
"=== outside the container (medium IL):"
& "$copy\ws.exe" probe $copy 2>&1
"=== inside the container, no capability:"
& $spike run --name kulespike1 --grant $copy --cwd $copy -- "$copy\ws.exe" probe $copy 2>&1 | Where-Object { $_ -notmatch "Successfully processed" }
"=== inside, with internetClient capability:"
& $spike run --name kulespike1 --grant $copy --cwd $copy --cap S-1-15-3-1 -- "$copy\ws.exe" probe $copy 2>&1 | Where-Object { $_ -notmatch "Successfully processed" -and $_ -match "tcp|exit" }
Stop-Job $listener; Remove-Job $listener -Force
