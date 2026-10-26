# Runs a script in ANOTHER user's interactive desktop session (that user must be logged on at the
# console) with a limited token, through a scheduled task registered by the administrator, and
# prints its log. Used for measurements as the standard user kulespike, who cannot use the
# ScheduledTasks module (CIM access denied) to register a task for itself.
param([Parameter(Mandatory)][string]$Script, [string]$User = 'kulespike', [int]$TimeoutSec = 900)
$log = "$Script.$User.log"; Remove-Item $log -ErrorAction SilentlyContinue
$name = "KuleSpike-$User"
Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command `"& '$Script' *> '$log'`""
$principal = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSec) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings | Out-Null
Start-ScheduledTask -TaskName $name
$deadline = (Get-Date).AddSeconds($TimeoutSec)
do { Start-Sleep 2; $st = (Get-ScheduledTask -TaskName $name).State } while ($st -eq 'Running' -and (Get-Date) -lt $deadline)
"task state: $st, result: $((Get-ScheduledTaskInfo -TaskName $name).LastTaskResult)"
Get-Content $log -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $name -Confirm:$false
