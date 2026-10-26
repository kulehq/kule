# M10 (as a standard user) and M6: host facts, a first container, which
# environment entries PSEC requires, and what the default block contains.
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
New-Item -ItemType Directory -Force $w | Out-Null
$cmd = "C:\Windows\System32\cmd.exe"
"== probe as $env:USERNAME"
& $wm probe
"`n== 1. default environment (none supplied)"
& $wm run --rw $w --env-default --cwd $w -- $cmd /d /s /c "echo hello & set"
"`n== 2. only PATH supplied"
& $wm run --rw $w --env "PATH=$env:PATH" --cwd $w -- $cmd /d /s /c "set"
"`n== 3. Kule-like allowlist"
& $wm run --rw $w --env "PATH=$env:PATH" --env "SYSTEMROOT=$env:SystemRoot" --env "LOCALAPPDATA=$env:LOCALAPPDATA" --env "TEMP=$w" --env "TMP=$w" --env "HOME=$w" --env "USERPROFILE=$w" --cwd $w -- $cmd /d /s /c "set"
"`n== 4. empty list + inherit"
& $wm run --rw $w --env-inherit --cwd $w -- $cmd /d /s /c "set"
"`n== 5. proxy mode (identity-less) env injection"
& $wm run --rw $w --env "PATH=$env:PATH" --env "SYSTEMROOT=$env:SystemRoot" --env "LOCALAPPDATA=$env:LOCALAPPDATA" --net proxy-loopback:47700 --cwd $w -- $cmd /d /s /c "set | findstr /i proxy"
