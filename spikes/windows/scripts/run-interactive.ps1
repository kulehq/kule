# Runs a script in the person's interactive desktop session with a LIMITED (medium integrity) token,
# through a scheduled task, and prints its log. SSH sessions are elevated and have no interactive
# window station, which breaks programs that load user32 inside an AppContainer.
param([Parameter(Mandatory)][string]$Script, [int]$TimeoutSec = 900)
$log = "$Script.log"; Remove-Item $log -ErrorAction SilentlyContinue
$name = 'KuleSpike'
Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command `"& '$Script' *> '$log'`""
$principal = New-ScheduledTaskPrincipal -UserId (whoami) -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSec) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings | Out-Null
Start-ScheduledTask -TaskName $name
$deadline = (Get-Date).AddSeconds($TimeoutSec)
do { Start-Sleep 2; $st = (Get-ScheduledTask -TaskName $name).State } while ($st -eq 'Running' -and (Get-Date) -lt $deadline)
"task state: $st, result: $((Get-ScheduledTaskInfo -TaskName $name).LastTaskResult)"
Get-Content $log -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $name -Confirm:$false
