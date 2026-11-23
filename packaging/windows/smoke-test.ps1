# CI check of the unsigned MSIX: install it as a sideloaded package, confirm Windows registered
# it with its file associations, start the installed app once (--version) and remove it again.
param([Parameter(Mandatory)][string]$Msix)
$ErrorActionPreference = 'Stop'
if (-not (Get-Command Add-AppxPackage).Parameters.ContainsKey('AllowUnsigned')) {
    Write-Output 'SKIPPED: this Windows has no -AllowUnsigned, so it cannot install an unsigned MSIX'
    exit 0
}
Add-AppxPackage -Path $Msix -AllowUnsigned
$pkg = Get-AppxPackage -Name io.github.blub3.Huehuecoyotl
if (-not $pkg) { throw 'The package is not registered' }
Write-Output "installed: $($pkg.PackageFullName)"
try {
    $exe = Join-Path $pkg.InstallLocation 'bin\coyotl.exe'
    $log = Join-Path ([IO.Path]::GetTempPath()) 'coyotl-version.txt'
    # Without this, Qt shows --version in a message box and the GUI app waits for a click forever.
    $env:QT_COMMAND_LINE_PARSER_NO_GUI_MESSAGE_BOXES = '1'
    Remove-Item Env:QT_QPA_PLATFORM -ErrorAction SilentlyContinue # offscreen brings the box back
    $p = Start-Process $exe -ArgumentList '--version' -PassThru -NoNewWindow -RedirectStandardOutput $log
    $null = $p.Handle # keeps ExitCode readable after the process ends
    if (-not $p.WaitForExit(60000)) {
        Stop-Process -Id $p.Id -Force
        throw 'The installed app did not answer --version within 60 seconds'
    }
    $out = Get-Content $log -Raw
    Write-Output "coyotl --version: $out (exit $($p.ExitCode))"
    if ($p.ExitCode -ne 0 -or $out -notmatch 'coyotl \d') { throw 'The installed app did not start' }
} finally {
    Remove-AppxPackage -Package $pkg.PackageFullName
}
