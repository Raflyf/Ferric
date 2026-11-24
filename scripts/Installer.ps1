#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$AppExe,
    [string]$MakepadWidgetsResources
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $PSBoundParameters.ContainsKey('OutputDirectory')) { $OutputDirectory = Join-Path $root 'dist' }
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { throw 'OutputDirectory cannot be empty.' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)

$cargo = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"') {
    throw 'Installer version in Cargo.toml must be a numeric X.Y.Z version.'
}
$version = $Matches[1]
$output = Join-Path $OutputDirectory "ferricsetup-v$version.msi"
if (Test-Path -LiteralPath $output) {
    throw "Existing installer left untouched: $output. Choose another -OutputDirectory."
}

$exe = if ($PSBoundParameters.ContainsKey('AppExe')) {
    (Resolve-Path -LiteralPath $AppExe).ProviderPath
} else {
    Join-Path $root 'target\release\ferric.exe'
}
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "Release executable missing: $exe. Run cargo build --release -p ferric-desktop first."
}
foreach ($requiredAsset in @(
    'assets\brand\ferric.ico',
    'assets\brand\wix-banner.bmp',
    'assets\brand\wix-dialog.bmp',
    'installer\ONLINE-NOTICE.txt',
    'installer\Ferric.wxs'
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $requiredAsset) -PathType Leaf)) {
        throw "Required installer asset missing: $requiredAsset"
    }
}

# Locate Makepad widgets runtime resources (IBMPlexSans fonts + SVG icons)
if (-not $PSBoundParameters.ContainsKey('MakepadWidgetsResources') -or [string]::IsNullOrWhiteSpace($MakepadWidgetsResources)) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $regSrc = Join-Path $cargoHome 'registry\src'
    $MakepadWidgetsResources = Get-ChildItem -LiteralPath $regSrc -Recurse -Directory -Filter 'makepad-widgets-*' |
        ForEach-Object { Join-Path $_.FullName 'resources' } |
        Where-Object { Test-Path -LiteralPath $_ -PathType Container } |
        Select-Object -First 1
}
if (-not $MakepadWidgetsResources -or -not (Test-Path -LiteralPath (Join-Path $MakepadWidgetsResources 'IBMPlexSans-Text.ttf') -PathType Leaf)) {
    throw "Could not locate makepad-widgets resources (IBMPlexSans-Text.ttf missing): $MakepadWidgetsResources"
}

function Find-WixBinDir {
    $candidates = @()
    if (-not [string]::IsNullOrWhiteSpace($env:WIX)) {
        $candidates += (Join-Path $env:WIX 'bin')
    }
    if (-not [string]::IsNullOrWhiteSpace(${env:ProgramFiles(x86)})) {
        $candidates += (Join-Path ${env:ProgramFiles(x86)} 'WiX Toolset v3.14\bin')
    }
    foreach ($dir in $candidates) {
        if ((Test-Path -LiteralPath (Join-Path $dir 'candle.exe') -PathType Leaf) -and (Test-Path -LiteralPath (Join-Path $dir 'light.exe') -PathType Leaf)) {
            return $dir
        }
    }
    $cmd = Get-Command candle.exe -CommandType Application -ErrorAction SilentlyContinue
    if ($cmd) {
        $bin = Split-Path -Parent $cmd.Source
        if (Test-Path -LiteralPath (Join-Path $bin 'light.exe') -PathType Leaf) {
            return $bin
        }
    }
    return $null
}

function New-DeterministicGuid {
    param([string]$Seed)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes("Raflyf.Ferric.WiX::$Seed"))
        $guidBytes = New-Object byte[] 16
        [Array]::Copy($bytes, 0, $guidBytes, 0, 16)
        $guidBytes[7] = ($guidBytes[7] -band 0x0F) -bor 0x50
        $guidBytes[8] = ($guidBytes[8] -band 0x3F) -bor 0x80
        return ([Guid]::new($guidBytes)).ToString('D').ToUpperInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Escape-XmlAttr {
    param([string]$Value)
    return [System.Security.SecurityElement]::Escape($Value)
}

$tools = Join-Path $root 'scratch\tools'
$wixBin = Find-WixBinDir
if (-not $wixBin) {
    $archive = Join-Path $tools 'wix314-binaries.zip'
    $archiveHash = '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31'
    $candleHash = 'A464676E2A2191D58E6CA643E89505BFB9628D3E721873A14994D3F4667987E5'
    $lightHash = '8EE3D7FA5592511152A4D1FC605780EAE1B27AC7B41D75D518C958515DE39529'
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
        New-Item -ItemType Directory -Path $tools -Force | Out-Null
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip' -OutFile $archive -TimeoutSec 120
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $archiveHash) {
        throw "WiX archive checksum mismatch; left untouched: $archive"
    }
    $wixBin = Join-Path $tools ('wix-3.14-build-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $wixBin | Out-Null
    tar.exe -xf $archive -C $wixBin
    if ($LASTEXITCODE -ne 0) { throw "WiX extraction failed (exit $LASTEXITCODE). Retained: $wixBin" }
    if ((Get-FileHash -LiteralPath (Join-Path $wixBin 'candle.exe') -Algorithm SHA256).Hash -ne $candleHash) {
        throw 'Portable candle.exe checksum mismatch.'
    }
    if ((Get-FileHash -LiteralPath (Join-Path $wixBin 'light.exe') -Algorithm SHA256).Hash -ne $lightHash) {
        throw 'Portable light.exe checksum mismatch.'
    }
}

$candle = Join-Path $wixBin 'candle.exe'
$light = Join-Path $wixBin 'light.exe'

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$staging = Join-Path $tools ('installer-app-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
$packagedExe = Join-Path $staging 'ferric.exe'
[IO.File]::Copy($exe, $packagedExe, $false)

# Stage Makepad runtime resources under makepad/ for MAKEPAD_PACKAGE_DIR=makepad
$makepadStage = Join-Path $staging 'makepad'
$mwDest = Join-Path $makepadStage 'makepad_widgets\resources'
$fdDest = Join-Path $makepadStage 'ferric_desktop\resources'
New-Item -ItemType Directory -Path $mwDest -Force | Out-Null
New-Item -ItemType Directory -Path $fdDest -Force | Out-Null
Copy-Item (Join-Path $MakepadWidgetsResources '*') $mwDest -Recurse -Force
Copy-Item (Join-Path $root 'apps\ferric-desktop\resources\*') $fdDest -Force

# Generate ICE38/ICE64-compliant per-user WiX fragment for all staged makepad/ resources
$dirIds = @{}
$dirLines = New-Object System.Collections.Generic.List[string]
$compLines = New-Object System.Collections.Generic.List[string]
$refLines = New-Object System.Collections.Generic.List[string]
$dirCounter = 0
$fileCounter = 0

function Add-ResourceDirectoryXml {
    param(
        [string]$FullDir,
        [string]$RelPath,
        [int]$Indent
    )
    $script:dirCounter++
    $dirId = "MpDir_$($script:dirCounter)"
    $script:dirIds[$RelPath] = $dirId
    $dirName = Split-Path -Leaf $FullDir
    $pad = ' ' * $Indent
    $script:dirLines.Add("$pad<Directory Id=`"$dirId`" Name=`"$(Escape-XmlAttr $dirName)`">")

    $subDirs = Get-ChildItem -LiteralPath $FullDir -Directory | Sort-Object Name
    foreach ($sd in $subDirs) {
        $childRel = if ($RelPath) { "$RelPath/$($sd.Name)" } else { $sd.Name }
        Add-ResourceDirectoryXml -FullDir $sd.FullName -RelPath $childRel -Indent ($Indent + 2)
    }
    $script:dirLines.Add("$pad</Directory>")

    # Component for this folder (files + HKCU KeyPath + RemoveFolder for ICE38/ICE64 compliance)
    $compId = "MpComp_$($script:dirCounter)"
    $guid = New-DeterministicGuid "dir:$RelPath"
    $regName = "Dir_$($script:dirCounter)"
    $script:refLines.Add("      <ComponentRef Id=`"$compId`" />")
    $script:compLines.Add("    <DirectoryRef Id=`"$dirId`">")
    $script:compLines.Add("      <Component Id=`"$compId`" Guid=`"$guid`" Win64=`"yes`">")
    $script:compLines.Add("        <RegistryValue Root=`"HKCU`" Key=`"Software\Raflyf\Ferric\Install\Resources`" Name=`"$regName`" Type=`"integer`" Value=`"1`" KeyPath=`"yes`" />")
    $files = Get-ChildItem -LiteralPath $FullDir -File | Sort-Object Name
    foreach ($f in $files) {
        $script:fileCounter++
        $fileId = "MpFile_$($script:fileCounter)"
        $script:compLines.Add("        <File Id=`"$fileId`" Source=`"$(Escape-XmlAttr $f.FullName)`" KeyPath=`"no`" />")
    }
    $script:compLines.Add("        <RemoveFolder Id=`"Rm_$dirId`" On=`"uninstall`" />")
    $script:compLines.Add("      </Component>")
    $script:compLines.Add("    </DirectoryRef>")
}

Add-ResourceDirectoryXml -FullDir $makepadStage -RelPath 'makepad' -Indent 6

$resWxs = Join-Path $staging 'MakepadResources.wxs'
$resXml = @"
<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <DirectoryRef Id="INSTALLFOLDER">
$($dirLines -join "`r`n")
    </DirectoryRef>
$($compLines -join "`r`n")
    <ComponentGroup Id="MakepadResourcesGroup">
$($refLines -join "`r`n")
    </ComponentGroup>
  </Fragment>
</Wix>
"@
[IO.File]::WriteAllText($resWxs, $resXml, [Text.Encoding]::UTF8)

# Generate combined RTF license notice (ONLINE-NOTICE.txt + LICENSE)
$noticeText = (Get-Content -LiteralPath (Join-Path $root 'installer\ONLINE-NOTICE.txt') -Raw).Trim()
$licenseText = (Get-Content -LiteralPath (Join-Path $root 'LICENSE') -Raw).Trim()
$combinedText = "$noticeText`r`n`r`n---`r`n`r`n$licenseText"
$escapedRtf = $combinedText.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}').Replace("`r`n", "`n").Replace("`n", "\par`r`n")
$licenseRtf = Join-Path $staging 'license.rtf'
[IO.File]::WriteAllText($licenseRtf, "{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\fs18 $escapedRtf}", [Text.Encoding]::ASCII)

$mainObj = Join-Path $staging 'Ferric.wixobj'
$resObj = Join-Path $staging 'MakepadResources.wixobj'
$wxs = Join-Path $root 'installer\Ferric.wxs'

& $candle -nologo -arch x64 -ext WixUIExtension -ext WixUtilExtension "-dAppVersion=$version" "-dAppExe=$packagedExe" "-dRepoRoot=$root" "-dLicenseRtf=$licenseRtf" -out $mainObj $wxs
if ($LASTEXITCODE -ne 0) { throw "WiX candle compilation failed for Ferric.wxs (exit $LASTEXITCODE)." }

& $candle -nologo -arch x64 -out $resObj $resWxs
if ($LASTEXITCODE -ne 0) { throw "WiX candle compilation failed for MakepadResources.wxs (exit $LASTEXITCODE)." }

& $light -nologo -ext WixUIExtension -ext WixUtilExtension -out $output $mainObj $resObj
if ($LASTEXITCODE -ne 0) { throw "WiX light linking failed (exit $LASTEXITCODE)." }

if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
    throw "Compiler produced no installer: $output"
}

if (-not ([Management.Automation.PSTypeName]'FerricMsiReader').Type) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class FerricMsiReader {
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    static extern uint MsiOpenDatabase(string szDatabasePath, IntPtr szPersist, out IntPtr phDatabase);
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    static extern uint MsiDatabaseOpenView(IntPtr hDatabase, string szQuery, out IntPtr phView);
    [DllImport("msi.dll")]
    static extern uint MsiViewExecute(IntPtr hView, IntPtr hRecord);
    [DllImport("msi.dll")]
    static extern uint MsiViewFetch(IntPtr hView, out IntPtr phRecord);
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    static extern uint MsiRecordGetString(IntPtr hRecord, uint iField, StringBuilder szValueBuf, ref uint pcchValueBuf);
    [DllImport("msi.dll")]
    static extern uint MsiCloseHandle(IntPtr hAny);

    public static string GetProperty(string msiPath, string name) {
        IntPtr db = IntPtr.Zero, view = IntPtr.Zero, rec = IntPtr.Zero;
        try {
            if (MsiOpenDatabase(msiPath, IntPtr.Zero, out db) != 0) throw new Exception("MsiOpenDatabase failed.");
            if (MsiDatabaseOpenView(db, "SELECT `Value` FROM `Property` WHERE `Property` = '" + name + "'", out view) != 0) throw new Exception("MsiDatabaseOpenView failed.");
            if (MsiViewExecute(view, IntPtr.Zero) != 0) throw new Exception("MsiViewExecute failed.");
            if (MsiViewFetch(view, out rec) != 0 || rec == IntPtr.Zero) return "";
            uint len = 512;
            var sb = new StringBuilder((int)len);
            if (MsiRecordGetString(rec, 1, sb, ref len) != 0) return "";
            return sb.ToString();
        } finally {
            if (rec != IntPtr.Zero) MsiCloseHandle(rec);
            if (view != IntPtr.Zero) MsiCloseHandle(view);
            if (db != IntPtr.Zero) MsiCloseHandle(db);
        }
    }
}
'@
}

$msiVersion = ([FerricMsiReader]::GetProperty($output, 'ProductVersion')).Trim()
$msiName = ([FerricMsiReader]::GetProperty($output, 'ProductName')).Trim()
$msiManufacturer = ([FerricMsiReader]::GetProperty($output, 'Manufacturer')).Trim()
if ($msiVersion -ne $version -or $msiName -ne 'Ferric' -or $msiManufacturer -ne 'Raflyf') {
    throw "Installer MSI metadata mismatch: Name=$msiName, Manufacturer=$msiManufacturer, Version=$msiVersion, expected=$version."
}

$artifact = Get-Item -LiteralPath $output
Write-Output "MSI Product: $msiName ($msiManufacturer); ProductVersion: $msiVersion"
Write-Output "Installer: $output"
Write-Output "Bytes: $($artifact.Length)"
Write-Output "SHA256: $((Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash)"
