# Executables outside System32, directory enumeration, batch files by path.
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
Remove-Item "$w\rw\git.cmd" -Force -ErrorAction SilentlyContinue
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
function T($label, $a) { "`n== $label"; & $wm run @a 2>&1 | Select-Object -First 14 }
T "i. git.exe full path, --ro Git" (@("--rw","$w\rw","--ro","C:\Program Files\Git") + $env + @("--cwd","$w\rw","--","C:\Program Files\Git\cmd\git.exe","--version"))
T "j. powershell (System32) echo" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe","-NoProfile","-c","echo ps-ran"))
T "m. curl.exe (System32) --version" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--net","deny","--","C:\Windows\System32\curl.exe","--version"))
T "k. node copied into rw, env default" (@("--rw","$w\rw","--env-default","--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "l. node copied into rw, ui enabled" (@("--rw","$w\rw","--ui") + $env + @("--cwd","$w\rw","--","$w\rw\node.exe","-v"))
T "n. notepad-free user32 user: whoami.exe" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--","C:\Windows\System32\whoami.exe","/groups"))
T "o. enumerate rw root" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw"))
T "p. enumerate rw\sub" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw\sub"))
T "q. enumerate rw root with --enum" (@("--rw","$w\rw","--enum","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw"))
T "r. batch by absolute path with --enum" (@("--rw","$w\rw","--enum","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","$w\rw\one.cmd"))
T "s. batch by relative name" (@("--rw","$w\rw") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","one.cmd"))
T "t. rw = parent, work in child" (@("--rw","$w") + $env + @("--cwd","$w\rw","--",$cmd,"/d","/s","/c","dir /b $w\rw && $w\rw\one.cmd"))
T "u. wm.exe from rw with ui, env default" (@("--rw","$w\rw","--ui","--env-default","--cwd","$w\rw","--","$w\rw\wm.exe","connect","127.0.0.1:1"))
