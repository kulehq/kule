# Enumeration of ancestors; do children of cmd die the same way?
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
$parents = @("--enum","C:\","--enum","C:\Users","--enum","C:\Users\kulespike","--enum","$w")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 10 }
T "1. dir rw with ancestors enumerable" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw"))
T "2. batch by absolute path with ancestors enumerable" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\one.cmd"))
T "3. cmd /c whoami" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","whoami & echo rc=%errorlevel%"))
T "4. cmd /c whoami, ui enabled" (@("--rw","$w\rw","--ui") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","whoami & echo rc=%errorlevel%"))
T "5. cmd /c node -v (rw copy)" (@("--rw","$w\rw","--ui") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\node.exe -v & echo rc=%errorlevel%"))
T "6. cmd /c hostname / findstr / where (System32 tools)" (@("--rw","$w\rw") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","hostname & echo rc=%errorlevel% & echo abc | findstr b & echo rc=%errorlevel% & where cmd & echo rc=%errorlevel% & tasklist /fi ""imagename eq cmd.exe"" /nh & echo rc=%errorlevel%"))
T "7. cmd /c wm.exe connect (rw copy)" (@("--rw","$w\rw","--ui") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\wm.exe connect 127.0.0.1:1 & echo rc=%errorlevel%"))
T "8. git.exe via cmd, --ro Git" (@("--rw","$w\rw","--ro","C:\Program Files\Git","--ui") + $parents + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","""C:\Program Files\Git\cmd\git.exe"" --version & echo rc=%errorlevel%"))
