# CI check of the unsigned MSIX: install it as a sideloaded package, confirm Windows registered
# it with its file associations, start the installed app once (--version) and remove it again.
param([Parameter(Mandatory)][string]$Msix)
$ErrorActionPreference = 'Stop'
Add-AppxPackage -Path $Msix -AllowUnsigned
$pkg = Get-AppxPackage -Name io.github.blub3.Huehuecoyotl
if (-not $pkg) { throw 'The package is not registered' }
Write-Output "installed: $($pkg.PackageFullName)"
try {
    $exe = Join-Path $pkg.InstallLocation 'bin\coyotl.exe'
    $log = Join-Path ([IO.Path]::GetTempPath()) 'coyotl-version.txt'
    $p = Start-Process $exe -ArgumentList '--version' -Wait -PassThru -NoNewWindow -RedirectStandardOutput $log
    $out = Get-Content $log -Raw
    Write-Output "coyotl --version: $out (exit $($p.ExitCode))"
    if ($p.ExitCode -ne 0 -or $out -notmatch 'coyotl \d') { throw 'The installed app did not start' }
} finally {
    Remove-AppxPackage -Package $pkg.PackageFullName
}
