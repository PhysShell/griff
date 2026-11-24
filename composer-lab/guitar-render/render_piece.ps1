param(
    [Parameter(Mandatory = $true)][string]$Mid,
    [string]$MusicXml = "",
    [ValidateSet("best", "heavy", "clean", "crunch", "texture")][string]$Variant = "best",
    [ValidateSet("strict", "expressive")][string]$Mode = "strict"
)
$ErrorActionPreference = "Stop"
$root = if ($env:GUITAR_RENDER_ROOT) { $env:GUITAR_RENDER_ROOT } else { [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\..\..\_griff_out\guitar-render-01")) }
$code = $PSScriptRoot
$Mid = (Resolve-Path $Mid).Path
$name = [IO.Path]::GetFileNameWithoutExtension($Mid) + "__" + $Variant + "__" + $Mode

$prep = Join-Path $root "prep\mine\$name"
$extra = @("--mode", $Mode, "--dead-velocity", "40", "--out", $prep)
if ($MusicXml) { $extra += @("--musicxml", (Resolve-Path $MusicXml).Path) }
switch ($Variant) {
    "heavy"   { $extra += @("--preset-all", "SWAN-HEAVY") }
    "clean"   { $extra += @("--preset-all", "SWAN-CLEAN") }
    "crunch"  { $extra += @("--preset-all", "SWAN-CRUNCH") }
    "texture" { $extra += @("--preset-all", "SWAN-TEXTURE") }
}
$ok = $false
foreach ($song in "owner_3tracks", "owner_2tracks") {
    if (Test-Path $prep) { Remove-Item -Recurse -Force $prep }
    & python "$code\prepare.py" --input $Mid --song $song @extra 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { $ok = $true; break }
}
if (-not $ok) { throw "prepare.py failed for both 3- and 2-track layouts" }

$w = { param($p) "/mnt/" + $p.Substring(0, 1).ToLower() + $p.Substring(2).Replace("\", "/") }
$raw = & $w (Join-Path $root "out\mine_raw\$name")
$job = (& $w "$code\jobs\render_job.sh")
wsl -d NixOS -- bash -lc "rm -rf '$raw' && REPEATS=1 bash '$job' '$raw' '$(& $w $prep)'"
if ($LASTEXITCODE -ne 0) { throw "render job failed" }

$dst = Join-Path $root "out\mine\$name"
New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item -Recurse -Force (Join-Path $root "out\mine_raw\$name\$name\run1\*") $dst
Write-Host "Done: $dst\guitars.mix.mp3"
