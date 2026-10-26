$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
$spike = 'C:\src\winspike\target\debug\winspike.exe'
$root = Join-Path $env:LOCALAPPDATA 'kule-spike'; $copy = "$root\copy"
New-Item -ItemType Directory -Force $copy | Out-Null
Copy-Item $spike "$copy\ws.exe" -Force; Copy-Item C:\Windows\System32\curl.exe "$copy\c.exe" -Force
$name = 'kulespike1'
function Run($label, $cmdline, $extra = @()) {
  Remove-Item "$copy\out.txt" -ErrorAction SilentlyContinue
  $p = Start-Process -FilePath $spike -ArgumentList (@('run','--name',$name,'--grant',$copy,'--cwd',$copy) + $extra + @('--') + $cmdline) -Wait -PassThru -NoNewWindow -RedirectStandardError "$root\err.txt" -RedirectStandardOutput "$root\so.txt"
  [string]$out = [string](Get-Content "$copy\out.txt" -Raw -ErrorAction SilentlyContinue) + [string](Get-Content "$root\so.txt" -Raw -ErrorAction SilentlyContinue)
  $out = ($out -replace '\s+', ' ').Trim(); if ($out.Length -gt 120) { $out = $out.Substring(0,120) + '…' }
  "{0,-44} exit={1,-12} {2}" -f $label, $p.ExitCode, $out
}
"--- sddl (from outside):"
"NUL:      " + (& $spike sddl '\\.\NUL')
"ws.exe:   " + (& $spike sddl "$copy\ws.exe")
"copy:     " + (& $spike sddl $copy)
"AC\Temp:  " + (& $spike sddl "$env:LOCALAPPDATA\Packages\$name\AC\Temp")
"cmd.exe:  " + (& $spike sddl 'C:\Windows\System32\cmd.exe')
"Git dir:  " + (& $spike sddl 'C:\Program Files\Git\cmd')
Run 'direct: ws.exe token'                   @("$copy\ws.exe",'token')
Run 'cmd: ws.exe token'                      @('cmd','/c',"($copy\ws.exe token) > $copy\out.txt 2>&1")
Run 'cmd: copied curl c.exe --version'       @('cmd','/c',"($copy\c.exe --version) > $copy\out.txt 2>&1")
icacls "$copy\ws.exe" /setintegritylevel L | Out-Null
Run 'cmd: ws.exe with LOW label'             @('cmd','/c',"($copy\ws.exe token) > $copy\out.txt 2>&1")
icacls $copy /grant '*S-1-15-2-1:(OI)(CI)(RX)' | Out-Null
Run 'cmd: ws.exe + ALL APP PACKAGES RX'      @('cmd','/c',"($copy\ws.exe token) > $copy\out.txt 2>&1")
Run 'cmd: dir %TEMP%'                        @('cmd','/c',"(dir /b %TEMP%) > $copy\out.txt 2>&1")
Run 'cmd: git with AAP on copy'              @('cmd','/c',"(git --version) > $copy\out.txt 2>&1")
Run 'direct: git.exe --version'              @('C:\Program Files\Git\cmd\git.exe','--version')
Run 'direct: mingw git.exe --version'        @('C:\Program Files\Git\mingw64\bin\git.exe','--version')
Run 'direct: node --version'                 @('C:\Program Files\nodejs\node.exe','--version')
Run 'direct: powershell 1+1'                 @('powershell','-NoProfile','-c','1+1')
icacls $copy /remove:g '*S-1-15-2-1' | Out-Null
