# Why do whoami/node die and why is `dir` refused? Ask MXC's denial capture.
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
New-Item -ItemType Directory -Force "$w\logs" | Out-Null
Remove-Item "$w\logs\*" -Force -ErrorAction SilentlyContinue
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
function T($label, $a) {
  "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 6; Start-Sleep -Milliseconds 500
  Get-ChildItem "$w\logs" -Filter *.json | Where-Object { $_.Name -notlike "*.verbose.json" } | ForEach-Object {
    "--- denials ($($_.Name)):"
    (Get-Content $_.FullName -Raw | ConvertFrom-Json).denials | ForEach-Object { "  $($_.accessType) $($_.resourceType) $($_.resource)" } | Select-Object -Unique
  }
  Remove-Item "$w\logs\*" -Force -ErrorAction SilentlyContinue
}
T "1. whoami, ui disabled" (@("--rw","$w\rw","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe"))
T "2. whoami, ui enabled" (@("--rw","$w\rw","--ui","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe"))
T "3. dir rw" (@("--rw","$w\rw","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw"))
T "4. node from rw, ui enabled" (@("--rw","$w\rw","--ui","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "5. cmd set (works) for comparison" (@("--rw","$w\rw","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","set > nul"))
T "6. powershell echo, ui enabled" (@("--rw","$w\rw","--ui","--capture","$w\logs\d.json") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe","-NoProfile","-c","echo ps-ran"))
