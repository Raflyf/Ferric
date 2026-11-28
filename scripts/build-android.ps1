#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$FastDev,
    [switch]$SkipTests,
    [switch]$SkipLint,
    [string]$VersionOverride,
    [string[]]$Abis = @('arm64-v8a', 'armeabi-v7a', 'x86_64')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

if ($FastDev) {
    $Abis = @('arm64-v8a')
}

$ws = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).ProviderPath
$gradlePath = Join-Path $ws 'apps\ferric-android\app\build.gradle.kts'
$gradleText = Get-Content -Raw -LiteralPath $gradlePath
if (-not [string]::IsNullOrWhiteSpace($VersionOverride)) {
    $appVersion = $VersionOverride.Trim()
} elseif ($gradleText -match '(?m)versionName\s*=\s*"(\d+\.\d+\.\d+)"') {
    $appVersion = $Matches[1]
} else {
    throw "Failed to read Android versionName from $gradlePath"
}

$androidHome = if (-not [string]::IsNullOrWhiteSpace($env:ANDROID_HOME)) {
    $env:ANDROID_HOME
} elseif (-not [string]::IsNullOrWhiteSpace($env:ANDROID_SDK_ROOT)) {
    $env:ANDROID_SDK_ROOT
} else {
    Join-Path $env:LOCALAPPDATA 'Android\Sdk'
}
if (-not (Test-Path -LiteralPath $androidHome -PathType Container)) {
    throw "Android SDK not found at '$androidHome'. Set ANDROID_HOME or install Android SDK."
}

$ndkRoot = Join-Path $androidHome 'ndk'
if (-not (Test-Path -LiteralPath $ndkRoot -PathType Container)) {
    throw "Android NDK directory not found under '$ndkRoot'."
}
$ndkDir = Get-ChildItem -LiteralPath $ndkRoot -Directory |
    Sort-Object {
        try { [version]$_.Name } catch { [version]'0.0.0' }
    } -Descending |
    Select-Object -First 1
if (-not $ndkDir) {
    throw "No Android NDK versions installed under '$ndkRoot'."
}
$ndkHome = $ndkDir.FullName
$llvmReadelf = Join-Path $ndkHome 'toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-readelf.exe'
if (-not (Test-Path -LiteralPath $llvmReadelf -PathType Leaf)) {
    throw "llvm-readelf.exe not found at '$llvmReadelf'."
}

$freeDrive = $null
foreach ($letter in @('S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'R', 'Q')) {
    if (-not (Test-Path "${letter}:\")) {
        $freeDrive = "${letter}:"
        break
    }
}
if (-not $freeDrive) {
    throw 'No free drive letter available for space-safe subst build.'
}

$prevAndroidHome = $env:ANDROID_HOME
$prevAndroidSdkRoot = $env:ANDROID_SDK_ROOT
$prevAndroidNdkHome = $env:ANDROID_NDK_HOME
$prevRustflags = $env:RUSTFLAGS
$prevEncodedRustflags = $env:CARGO_ENCODED_RUSTFLAGS

& subst $freeDrive $ws
if ($LASTEXITCODE -ne 0) {
    throw "Failed to map $freeDrive to $ws (exit $LASTEXITCODE)."
}

try {
    $env:ANDROID_HOME = $androidHome
    $env:ANDROID_SDK_ROOT = $androidHome
    $env:ANDROID_NDK_HOME = $ndkHome

    Push-Location "${freeDrive}\"

    Write-Host "[1/5] Building ferric-mobile-ffi for ABIs: $($Abis -join ', ')"
    $jniLibsDir = "${freeDrive}\apps\ferric-android\app\src\main\jniLibs"
    New-Item -ItemType Directory -Force -Path $jniLibsDir | Out-Null

    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $rustFlags = @(
        '-C', 'link-arg=-Wl,-z,max-page-size=16384',
        "--remap-path-prefix=${freeDrive}\=/s",
        "--remap-path-prefix=${freeDrive}/=/s",
        "--remap-path-prefix=$ws=/s",
        "--remap-path-prefix=$($ws.Replace('\', '/'))=/s",
        "--remap-path-prefix=$cargoHome=/c",
        "--remap-path-prefix=$($cargoHome.Replace('\', '/'))=/c",
        "--remap-path-prefix=$($env:USERPROFILE)=/u",
        "--remap-path-prefix=$($env:USERPROFILE.Replace('\', '/'))=/u"
    )
    if ($env:CARGO_TARGET_DIR) {
        $rustFlags += "--remap-path-prefix=$($env:CARGO_TARGET_DIR)=/t"
        $rustFlags += "--remap-path-prefix=$($env:CARGO_TARGET_DIR.Replace('\', '/'))=/t"
    }
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = $rustFlags -join ([char]0x1F)
    $ndkArgs = @('ndk')
    foreach ($abi in $Abis) {
        $ndkArgs += @('-t', $abi)
    }
    $ndkArgs += @(
        '--platform', '28',
        '-o', $jniLibsDir,
        'build'
    )
    if (-not $FastDev) {
        $ndkArgs += '--release'
    }
    $ndkArgs += @('-p', 'ferric-mobile-ffi', '--locked')
    & cargo @ndkArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo ndk build failed (exit $LASTEXITCODE)."
    }

    Write-Host '[2/5] Verifying 16 KB ELF LOAD alignment on 64-bit native libraries'
    foreach ($abi64 in @('arm64-v8a', 'x86_64')) {
        if ($Abis -notcontains $abi64) {
            continue
        }
        $soFile = Join-Path $jniLibsDir "$abi64\libferric_mobile_ffi.so"
        if (-not (Test-Path -LiteralPath $soFile -PathType Leaf)) {
            throw "Expected native library missing: $soFile"
        }
        $readelfOut = & $llvmReadelf -l $soFile
        if ($LASTEXITCODE -ne 0) {
            throw "llvm-readelf failed on $soFile (exit $LASTEXITCODE)."
        }
        $loadLines = @($readelfOut | Where-Object { $_ -match '^\s*LOAD\s+' })
        if ($loadLines.Count -eq 0) {
            throw "No ELF LOAD segments found in $soFile"
        }
        foreach ($line in $loadLines) {
            if ($line -match '0x([0-9a-fA-F]+)\s*$') {
                $alignVal = [Convert]::ToInt64($Matches[1], 16)
                if ($alignVal -lt 16384) {
                    throw "ELF LOAD segment in $soFile is not 16 KB aligned (align=0x$($Matches[1])): $line"
                }
            } else {
                throw "Could not parse alignment from LOAD line in $soFile`: $line"
            }
        }
        $sizeMb = [Math]::Round((Get-Item -LiteralPath $soFile).Length / 1MB, 2)
        Write-Host "  OK $abi64/libferric_mobile_ffi.so ($sizeMb MB, $($loadLines.Count) LOAD segments >= 16 KB aligned)"
    }

    Write-Host '[3/5] Generating UniFFI Kotlin bindings from host ferric-mobile-ffi'
    $env:RUSTFLAGS = $prevRustflags
    $env:CARGO_ENCODED_RUSTFLAGS = $prevEncodedRustflags
    & cargo build -p ferric-mobile-ffi --locked
    if ($LASTEXITCODE -ne 0) {
        throw "Host ferric-mobile-ffi build failed (exit $LASTEXITCODE)."
    }
    $meta = & cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $hostDll = Join-Path $meta.target_directory 'debug\ferric_mobile_ffi.dll'
    if (-not (Test-Path -LiteralPath $hostDll -PathType Leaf)) {
        throw "Host library not found at $hostDll"
    }
    $uniffiOut = "${freeDrive}\apps\ferric-android\app\build\generated\source\uniffi\kotlin"
    New-Item -ItemType Directory -Force -Path $uniffiOut | Out-Null
    & cargo run -p ferric-mobile-ffi --features bindgen-cli --bin uniffi-bindgen -- generate `
        --library $hostDll `
        --language kotlin `
        --out-dir $uniffiOut `
        --no-format
    if ($LASTEXITCODE -ne 0) {
        throw "uniffi-bindgen failed (exit $LASTEXITCODE)."
    }

    Write-Host '[4/5] Running Gradle verification and release dual-APK build'
    Push-Location "${freeDrive}\apps\ferric-android"
    $gradleTasks = @()
    if (-not $SkipTests) {
        $gradleTasks += 'testDebugUnitTest'
    }
    if (-not $SkipLint) {
        $gradleTasks += 'lintDebug'
    }
    if ($FastDev) {
        $gradleTasks += 'assembleDebug'
    } else {
        $gradleTasks += 'assembleRelease'
        $gradleTasks += '--no-daemon'
    }

    & .\gradlew.bat @gradleTasks
    if ($LASTEXITCODE -ne 0) {
        throw "Gradle build failed (exit $LASTEXITCODE)."
    }
    Pop-Location

    if ($FastDev) {
        $dbgApk = Join-Path $ws 'apps\ferric-android\app\build\outputs\apk\debug\app-arm64-v8a-debug.apk'
        if (Test-Path -LiteralPath $dbgApk -PathType Leaf) {
            $item = Get-Item -LiteralPath $dbgApk
            Write-Host ("[FastDev] Ready: {0} ({1} MB)" -f $item.FullName, [Math]::Round($item.Length / 1MB, 2))
        }
        return
    }

    Write-Host '[5/5] Staging APKs, verifying APK native contents, and writing SHA256SUMS.txt'
    $apkOutDir = Join-Path $ws 'apps\ferric-android\app\build\outputs\apk\release'
    $distDir = Join-Path $ws 'dist\android'
    New-Item -ItemType Directory -Force -Path $distDir | Out-Null

    $arm64Src = Join-Path $apkOutDir 'app-arm64-v8a-release.apk'
    $univSrc = Join-Path $apkOutDir 'app-universal-release.apk'
    if (-not (Test-Path -LiteralPath $arm64Src -PathType Leaf)) {
        throw "Missing arm64-v8a release APK at $arm64Src"
    }
    if (-not (Test-Path -LiteralPath $univSrc -PathType Leaf)) {
        throw "Missing universal release APK at $univSrc"
    }

    $arm64Name = "ferric-v$appVersion-android-arm64-v8a.apk"
    $univName = "ferric-v$appVersion-android-universal.apk"
    $arm64Dst = Join-Path $distDir $arm64Name
    $univDst = Join-Path $distDir $univName
    Copy-Item -LiteralPath $arm64Src -Destination $arm64Dst -Force
    Copy-Item -LiteralPath $univSrc -Destination $univDst -Force

    foreach ($check in @(
        @{ Path = $arm64Dst; Expected = @('lib/arm64-v8a/libferric_mobile_ffi.so', 'lib/arm64-v8a/libjnidispatch.so') },
        @{ Path = $univDst; Expected = @('lib/arm64-v8a/libferric_mobile_ffi.so', 'lib/armeabi-v7a/libferric_mobile_ffi.so', 'lib/x86_64/libferric_mobile_ffi.so', 'lib/arm64-v8a/libjnidispatch.so') }
    )) {
        $zip = [System.IO.Compression.ZipFile]::OpenRead($check.Path)
        try {
            $entryNames = @($zip.Entries | ForEach-Object { $_.FullName })
            foreach ($expectedEntry in $check.Expected) {
                if ($entryNames -notcontains $expectedEntry) {
                    throw "APK $($check.Path) is missing expected native entry: $expectedEntry"
                }
            }
        } finally {
            $zip.Dispose()
        }
    }

    $arm64Hash = (Get-FileHash -LiteralPath $arm64Dst -Algorithm SHA256).Hash.ToLowerInvariant()
    $univHash = (Get-FileHash -LiteralPath $univDst -Algorithm SHA256).Hash.ToLowerInvariant()
    $sumsText = "$arm64Hash  $arm64Name`n$univHash  $univName`n"
    $sumsPath = Join-Path $distDir 'SHA256SUMS.txt'
    [System.IO.File]::WriteAllText($sumsPath, $sumsText, [System.Text.Encoding]::ASCII)

    Write-Host ''
    Write-Host 'Android release artifacts ready:'
    foreach ($file in @($arm64Dst, $univDst)) {
        $item = Get-Item -LiteralPath $file
        $mb = [Math]::Round($item.Length / 1MB, 2)
        Write-Host ("  {0} : {1} MB ({2} bytes)" -f $item.Name, $mb, $item.Length)
    }
    Write-Host ''
    Write-Host (Get-Content -Raw -LiteralPath $sumsPath).TrimEnd()
} finally {
    while ((Get-Location).Path -like "${freeDrive}\*") {
        Pop-Location
    }
    & subst $freeDrive /D | Out-Null
    $env:ANDROID_HOME = $prevAndroidHome
    $env:ANDROID_SDK_ROOT = $prevAndroidSdkRoot
    $env:ANDROID_NDK_HOME = $prevAndroidNdkHome
    $env:RUSTFLAGS = $prevRustflags
    $env:CARGO_ENCODED_RUSTFLAGS = $prevEncodedRustflags
}
