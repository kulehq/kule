# E4: copy cost on NTFS vs a Dev Drive (ReFS) for a source tree. Admin.
$ErrorActionPreference = 'Continue'
$root = 'C:\kule-spike-copy'; New-Item -ItemType Directory -Force $root | Out-Null
if (-not (Test-Path V:\)) {
  $vhd = "$root\dev.vhdx"
  "create vdisk file=$vhd maximum=20480 type=expandable`nselect vdisk file=$vhd`nattach vdisk`ncreate partition primary`nassign letter=V" | Set-Content "$root\dp.txt" -Encoding ascii
  diskpart /s "$root\dp.txt" | Select-Object -Last 2
  Format-Volume -DriveLetter V -DevDrive -Confirm:$false | Select-Object FileSystemType, @{n='SizeGB';e={[int]($_.Size/1GB)}} | Format-List
}
fsutil devdrv query V: 2>&1 | Select-Object -First 2
fsutil fsinfo refsinfo V: 2>&1 | Select-String "Bytes Per Cluster|Version"
# dataset 1: kule source tree without target; dataset 2: cargo registry sources
$ds1 = "$root\ds1-src"; if (-not (Test-Path $ds1)) { robocopy C:\src\kule $ds1 /E /XD target .git /NFL /NDL /NJH /NJS /NP | Out-Null }
$ds2 = "$root\ds2-src"; if (-not (Test-Path $ds2)) { robocopy C:\Users\selcuk1\.cargo\registry\src $ds2 /E /NFL /NDL /NJH /NJS /NP | Out-Null }
foreach ($ds in $ds1, $ds2) { $m = Get-ChildItem $ds -Recurse -File | Measure-Object Length -Sum; "{0}: {1} files, {2} MB" -f (Split-Path $ds -Leaf), $m.Count, [int]($m.Sum/1MB) }
function Time($label, $block) { $t = [Diagnostics.Stopwatch]::StartNew(); & $block; "{0,-46} {1,8:N2} s" -f $label, $t.Elapsed.TotalSeconds }
foreach ($n in 'ds1','ds2') {
  $src = "$root\$n-src"
  Remove-Item -Recurse -Force "$root\$n-nt", "V:\$n-a", "V:\$n-b", "V:\$n-c" -ErrorAction SilentlyContinue
  Time "$n NTFS -> NTFS  Copy-Item (CopyFile)"      { Copy-Item -Recurse $src "$root\$n-nt" }
  Time "$n NTFS -> DevDrive first copy"             { Copy-Item -Recurse $src "V:\$n-a" }
  Time "$n DevDrive -> DevDrive Copy-Item"          { Copy-Item -Recurse "V:\$n-a" "V:\$n-b" }
  Time "$n DevDrive -> DevDrive robocopy /MT:8"     { robocopy "V:\$n-a" "V:\$n-c" /E /MT:8 /NFL /NDL /NJH /NJS /NP | Out-Null }
  "   V: used after copies: {0} MB" -f [int]((Get-Volume V).Size - (Get-Volume V).SizeRemaining)/1MB
}
