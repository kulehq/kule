# Why do executables outside System32 fail inside the container?
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
New-Item -ItemType Directory -Force "$w\rw\sub" | Out-Null
"hello" | Set-Content "$w\rw\f.txt"
"@echo off`r`necho batch-ran" | Set-Content "$w\rw\one.cmd" -Encoding Ascii
Copy-Item $wm "$w\rw\wm.exe" -Force
Copy-Item "C:\Program Files\nodejs\node.exe" "$w\rw\node.exe" -Force
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
function T($label, $a) { "`n== $label"; & $wm run @a }
T "a. type a file in rw (cwd root)" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","type $w\rw\f.txt"))
T "b. run a batch file in rw (cwd root)" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\one.cmd"))
T "c. dir with cwd = subdir" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw\sub","--",$cmd,"/d","/s","/c","cd & dir /b .."))
T "d. wm.exe copied into rw" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","$w\rw\wm.exe","connect","127.0.0.1:1"))
T "e. node.exe copied into rw" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "f. node from Program Files, --ro nodejs" (@("--rw","$w\rw","--ro","C:\Program Files\nodejs") + $env + @("--cwd","$w\rw","--","C:\Program Files\nodejs\node.exe","-v"))
T "g. node from Program Files, --ro C:\Program Files" (@("--rw","$w\rw","--ro","C:\Program Files") + $env + @("--cwd","$w\rw","--","C:\Program Files\nodejs\node.exe","-v"))
T "h. git --version, --ro Git" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","git --version"))
T "i. git --version via full path" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $env + @("--cwd","$w\rw","--","C:\Program Files\Git\cmd\git.exe","--version"))
T "j. powershell -c echo (System32)" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe","-NoProfile","-c","echo ps-ran"))
T "k. node copied into rw, env default" (@("--rw","$w\rw","--env-default","--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "l. node copied into rw, ui enabled" (@("--rw","$w\rw","--ui") + $env + @("--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "m. curl.exe (System32)" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--net","deny","--","C:\Windows\System32\curl.exe","--version"))
