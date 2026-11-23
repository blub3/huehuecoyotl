# Packages an installed Windows build (cmake --install + windeployqt) for release:
#   <Out>/Huehuecoyotl-<version>-windows-x64.zip    portable folder, nothing to install
#   <Out>/Huehuecoyotl-<version>-windows-x64.msix   unsigned MSIX (see docs/user/README.md)
#   <Out>/THIRD_PARTY_NOTICES-windows.txt
#   <Out>/winget/                                   manifests for a later winget submission
# The version comes from the root CMakeLists.txt. Needs the Windows SDK (makeappx, makepri).
#
#   pwsh packaging/windows/package.ps1 -App dist/Huehuecoyotl -Out release
param(
    [Parameter(Mandatory)][string]$App,
    [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$App = (Resolve-Path $App).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path

$m = [regex]::Match((Get-Content "$root/CMakeLists.txt" -Raw), 'project\(Huehuecoyotl VERSION (\d+\.\d+\.\d+)')
if (-not $m.Success) { throw 'No version in CMakeLists.txt' }
$version = $m.Groups[1].Value
$base = "Huehuecoyotl-$version-windows-x64"
Write-Output "Huehuecoyotl $version"

# Portable zip: one folder "Huehuecoyotl" with bin\coyotl.exe inside.
$zip = Join-Path $Out "$base.zip"
Compress-Archive -Path $App -DestinationPath $zip -Force
Copy-Item "$App/THIRD_PARTY_NOTICES.txt" (Join-Path $Out 'THIRD_PARTY_NOTICES-windows.txt')

# MSIX: the same files, the image assets, the manifest and a resource index (resources.pri),
# which lets Windows pick the right icon size from the scale-* and targetsize-* variants.
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" |
    Sort-Object FullName | Select-Object -Last 1
if (-not $sdk) { throw 'makeappx.exe not found (Windows SDK)' }
$tools = $sdk.DirectoryName
$work = Join-Path ([IO.Path]::GetTempPath()) "coyotl-msix-$PID"
$stage = Join-Path $work 'package'
New-Item -ItemType Directory -Force $work | Out-Null
Copy-Item -Recurse $App $stage
Copy-Item -Recurse "$root/packaging/windows/msix/Assets" (Join-Path $stage 'Assets')
(Get-Content "$root/packaging/windows/msix/AppxManifest.xml.in" -Raw).Replace('@VERSION@', $version) |
    Set-Content -Encoding utf8 (Join-Path $stage 'AppxManifest.xml')
$priconfig = Join-Path $work 'priconfig.xml'
& "$tools\makepri.exe" createconfig /cf $priconfig /dq en-US /o | Out-Null
if ($LASTEXITCODE) { throw "makepri createconfig failed ($LASTEXITCODE)" }
& "$tools\makepri.exe" new /pr $stage /cf $priconfig /of (Join-Path $stage 'resources.pri') /o | Out-Null
if ($LASTEXITCODE) { throw "makepri new failed ($LASTEXITCODE)" }
$msix = Join-Path $Out "$base.msix"
& "$tools\makeappx.exe" pack /d $stage /p $msix /o | Out-Null
if ($LASTEXITCODE) { throw "makeappx pack failed ($LASTEXITCODE)" }
Remove-Item -Recurse -Force $work

# winget manifests for the portable zip. Submitting them needs a public download URL, so this
# waits until the repository is public; the files are kept with each release for that day.
$sha = (Get-FileHash $zip -Algorithm SHA256).Hash
$id = 'blub3.Huehuecoyotl'
$url = "https://github.com/blub3/huehuecoyotl/releases/download/v$version/$base.zip"
$wg = Join-Path $Out 'winget'
New-Item -ItemType Directory -Force $wg | Out-Null
@"
PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
"@ | Set-Content -Encoding utf8 (Join-Path $wg "$id.yaml")
@"
PackageIdentifier: $id
PackageVersion: $version
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: Huehuecoyotl\bin\coyotl.exe
    PortableCommandAlias: coyotl
Installers:
  - Architecture: x64
    InstallerUrl: $url
    InstallerSha256: $sha
ManifestType: installer
ManifestVersion: 1.6.0
"@ | Set-Content -Encoding utf8 (Join-Path $wg "$id.installer.yaml")
@"
PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: The Huehuecoyotl contributors
PackageName: Huehuecoyotl
PackageUrl: https://github.com/blub3/huehuecoyotl
License: Apache-2.0
ShortDescription: Play and browse your films and series from local folders and network shares.
Tags: [video, media-player, mpv, nas, smb]
ManifestType: defaultLocale
ManifestVersion: 1.6.0
"@ | Set-Content -Encoding utf8 (Join-Path $wg "$id.locale.en-US.yaml")

Get-ChildItem -Recurse $Out | ForEach-Object { '{0,12:N0}  {1}' -f $_.Length, $_.FullName.Substring($Out.Length + 1) }
