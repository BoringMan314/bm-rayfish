[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$RayExe,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [string]$OutputZip
)

$ErrorActionPreference = 'Stop'

function Get-FullPathSafe([string]$Path) {
    $Path = $Path.Trim().Trim('"').TrimEnd('\', '/')
    if ($Path -match '^[A-Za-z]:$') {
        $Path = $Path + '\'
    }
    return [System.IO.Path]::GetFullPath($Path)
}

# Pinned Wintun, byte-identical to scripts/build-windows-msi.ps1.
$wintunUrl = 'https://www.wintun.net/builds/wintun-0.14.1.zip'
$wintunSha256 = '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51'

$ray = Get-FullPathSafe $RayExe
$zip = Get-FullPathSafe $OutputZip
$payloadName = "bm-rayfish.exe"
if (-not (Test-Path -LiteralPath $ray -PathType Leaf)) {
    throw "ray.exe was not found at $ray"
}

$cacheDir = Join-Path $env:LOCALAPPDATA 'rayfish-wintun\0.14.1'
$cachedDll = Join-Path $cacheDir 'amd64\wintun.dll'
if (-not (Test-Path -LiteralPath $cachedDll -PathType Leaf)) {
    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "rayfish-portable-wintun-$PID"
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    try {
        $archive = Join-Path $tempRoot 'wintun-0.14.1.zip'
        Write-Host "Downloading Wintun 0.14.1 for portable zip..."
        Invoke-WebRequest -Uri $wintunUrl -OutFile $archive -UseBasicParsing
        $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($archiveHash -ne $wintunSha256.ToLowerInvariant()) {
            throw "Wintun archive SHA-256 mismatch: expected $wintunSha256, got $archiveHash."
        }
        $extractRoot = Join-Path $tempRoot 'extract'
        Expand-Archive -LiteralPath $archive -DestinationPath $extractRoot -Force
        $dll = Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter 'wintun.dll' |
            Where-Object { $_.FullName -match '\\amd64\\wintun\.dll$' } |
            Select-Object -First 1
        if (-not $dll) {
            throw 'amd64/wintun.dll was not found in the pinned Wintun archive.'
        }
        $signature = Get-AuthenticodeSignature -FilePath $dll.FullName
        if ($signature.Status -ne 'Valid') {
            throw "Wintun Authenticode signature is not valid: $($signature.Status)."
        }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $cachedDll) | Out-Null
        Copy-Item -LiteralPath $dll.FullName -Destination $cachedDll -Force
    }
    finally {
        if (Test-Path -LiteralPath $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

$stage = Join-Path ([System.IO.Path]::GetTempPath()) "rayfish-portable-stage-$PID"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
try {
    Copy-Item -LiteralPath $ray -Destination (Join-Path $stage $payloadName) -Force
    Copy-Item -LiteralPath $cachedDll -Destination (Join-Path $stage 'wintun.dll') -Force
    $zipDir = Split-Path -Parent $zip
    if ($zipDir -and -not (Test-Path -LiteralPath $zipDir)) {
        New-Item -ItemType Directory -Force -Path $zipDir | Out-Null
    }
    if (Test-Path -LiteralPath $zip) {
        Remove-Item -LiteralPath $zip -Force
    }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
}
finally {
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if (-not (Test-Path -LiteralPath $zip -PathType Leaf)) {
    throw "portable zip was not written to $zip"
}
Write-Output "ZIP: $zip"
