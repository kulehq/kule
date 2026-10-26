# M2 listeners reachable from the host, M1 ConPTY inside, M4 tools (git, node).
$wm = "C:\kule-setup\wm.exe"
$w = "$env:USERPROFILE\mxcwork"
$cmd = "C:\Windows\System32\cmd.exe"
$node = "C:\Program Files\nodejs\node.exe"
New-Item -ItemType Directory -Force "$w\rw" | Out-Null
$env = @("--env","PATH=$env:PATH","--env","SYSTEMROOT=$env:SystemRoot","--env","LOCALAPPDATA=$env:LOCALAPPDATA")
$server = "require('http').createServer((q,s)=>s.end('ok')).listen(18090,'127.0.0.1',()=>console.log('listening'))"
foreach ($net in @("deny","proxy-loopback:47700","allow")) {
  "`n== M2 listener inside, host connects; net=$net"
  & $wm run --rw "$w\rw" --ro "C:\Program Files\nodejs" @env --net $net --cwd "$w\rw" --probe-port 18090 --after 2500 --timeout 15000 -- "`"$node`"" -e "`"$server`""
}
"`n== M1 ConPTY inside (ui disabled)"
& $wm run --rw "$w\rw" --ro "C:\kule-setup" @env --cwd "$w\rw" --timeout 20000 -- $wm conpty-host -- $cmd /d /s /c "echo conpty-works"
"`n== M1b ConPTY inside (ui enabled)"
& $wm run --rw "$w\rw" --ro "C:\kule-setup" @env --ui --cwd "$w\rw" --timeout 20000 -- $wm conpty-host -- $cmd /d /s /c "echo conpty-works"
"`n== M4 git in a fresh repo inside the copy"
Remove-Item -Recurse -Force "$w\rw\repo" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$w\rw\repo" | Out-Null
"hello" | Set-Content "$w\rw\repo\a.txt"
$git = @"
cd /d $w\rw\repo
git init -q . && echo [init ok] || echo [init FAIL]
git -c user.email=a@b -c user.name=t add . && git -c user.email=a@b -c user.name=t commit -qm one && echo [commit ok] || echo [commit FAIL]
git status --short && echo [status ok] || echo [status FAIL]
git log --oneline | find /c /v ""
"@
$git | Set-Content "$w\rw\git.cmd" -Encoding Ascii
& $wm run --rw "$w\rw" --ro "C:\Program Files\Git" @env --cwd "$w\rw\repo" -- $cmd /d /s /c "$w\rw\git.cmd"
"`n== M4 git with HOME/USERPROFILE pointing into rw (config lookups)"
& $wm run --rw "$w\rw" --ro "C:\Program Files\Git" @env --env "HOME=$w\rw" --env "USERPROFILE=$w\rw" --cwd "$w\rw\repo" -- $cmd /d /s /c "git status --short && echo [status ok] || echo [status FAIL]"
"`n== M4 node resolving upward and npm"
New-Item -ItemType Directory -Force "$w\rw\proj" | Out-Null
'{"name":"p","version":"1.0.0","private":true}' | Set-Content "$w\rw\proj\package.json"
"console.log('node ok', process.cwd())" | Set-Content "$w\rw\proj\i.js"
& $wm run --rw "$w\rw" --ro "C:\Program Files\nodejs" @env --env "APPDATA=$w\rw\appdata" --cwd "$w\rw\proj" --timeout 120000 -- $cmd /d /s /c "node i.js && npm --version && npm install --no-audit --no-fund left-pad@1.3.0 && dir /b node_modules && echo [npm ok] || echo [npm FAIL]"
