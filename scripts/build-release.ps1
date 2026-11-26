#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$TargetDir
)

$ErrorActionPreference = 'Stop'
$ws = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).ProviderPath
$wsSlash = $ws.Replace('\', '/')
$origCargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$userProf = $env:USERPROFILE
$userSlash = $userProf.Replace('\', '/')

# Pick free temporary subst drive letters so absolute workspace or user-profile paths
# never enter COFF/DWARF/panic strings or proc-macro CARGO_MANIFEST_DIR expansions.
$freeLetters = @()
foreach ($letter in @('S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'R', 'Q')) {
    if (-not (Test-Path "${letter}:\")) {
        $freeLetters += "${letter}:"
    }
}
if ($freeLetters.Count -lt 1) {
    throw 'No free drive letter available for isolated subst build.'
}
$substDrive = $freeLetters[0]
$cargoDrive = $null

$prevMakepadDir = $env:MAKEPAD_PACKAGE_DIR
$prevCflags = $env:CFLAGS
$prevCxxflags = $env:CXXFLAGS
$prevRustflags = $env:CARGO_ENCODED_RUSTFLAGS
$prevTargetDir = $env:CARGO_TARGET_DIR
$prevCargoHome = $env:CARGO_HOME

& subst $substDrive $ws
if ($LASTEXITCODE -ne 0) {
    throw "Failed to map $substDrive to $ws (exit $LASTEXITCODE)."
}

$activeCargoHome = $origCargoHome
if ($origCargoHome -match '(?i)\\users\\' -and (Test-Path -LiteralPath $origCargoHome -PathType Container) -and $freeLetters.Count -ge 2) {
    $cargoParent = Split-Path -Parent $origCargoHome
    $cargoLeaf = Split-Path -Leaf $origCargoHome
    if (-not [string]::IsNullOrWhiteSpace($cargoParent) -and -not [string]::IsNullOrWhiteSpace($cargoLeaf)) {
        $cargoDrive = $freeLetters[1]
        & subst $cargoDrive $cargoParent
        if ($LASTEXITCODE -ne 0) {
            & subst $substDrive /D | Out-Null
            throw "Failed to map $cargoDrive to $cargoParent (exit $LASTEXITCODE)."
        }
        $activeCargoHome = "${cargoDrive}\$cargoLeaf"
    }
}

try {
    Push-Location "${substDrive}\"
    $env:MAKEPAD_PACKAGE_DIR = 'makepad'
    $env:CARGO_HOME = $activeCargoHome
    if (-not [string]::IsNullOrWhiteSpace($TargetDir)) {
        $env:CARGO_TARGET_DIR = $TargetDir
    }
    $cargoTrim = $activeCargoHome.TrimEnd('\')
    $cargoSlash = $activeCargoHome.Replace('\', '/')
    $origCargoSlash = $origCargoHome.Replace('\', '/')
    $env:CFLAGS = "/guard:cf /d1trimfile:${substDrive}\ /d1trimfile:$cargoTrim /d1trimfile:$origCargoHome /d1trimfile:$userProf"
    $env:CXXFLAGS = "/guard:cf /d1trimfile:${substDrive}\ /d1trimfile:$cargoTrim /d1trimfile:$origCargoHome /d1trimfile:$userProf"

    $flags = @(
        '-C', 'control-flow-guard=yes',
        '-C', 'link-arg=/GUARD:CF',
        '-C', 'link-arg=/CETCOMPAT',
        '-C', 'link-arg=/DYNAMICBASE',
        '-C', 'link-arg=/HIGHENTROPYVA',
        '-C', 'link-arg=/NXCOMPAT',
        '-C', 'link-arg=/DEPENDENTLOADFLAG:0x800',
        '-C', 'link-arg=/DEBUG:NONE',
        '-C', 'link-arg=/NOCOFFGRPINFO',
        '-C', 'link-arg=/INCREMENTAL:NO',
        '-C', 'link-arg=/OPT:REF',
        '-C', 'link-arg=/OPT:ICF',
        '-C', 'link-arg=/BREPRO',
        "--remap-path-prefix=${substDrive}\=/s",
        "--remap-path-prefix=${substDrive}/=/s",
        "--remap-path-prefix=$ws=/s",
        "--remap-path-prefix=$wsSlash=/s",
        "--remap-path-prefix=$activeCargoHome=/c",
        "--remap-path-prefix=$cargoSlash=/c",
        "--remap-path-prefix=$origCargoHome=/c",
        "--remap-path-prefix=$origCargoSlash=/c",
        "--remap-path-prefix=$userProf=/u",
        "--remap-path-prefix=$userSlash=/u"
    )
    if ($env:CARGO_TARGET_DIR) {
        $tSlash = $env:CARGO_TARGET_DIR.Replace('\', '/')
        $flags += "--remap-path-prefix=$($env:CARGO_TARGET_DIR)=/t"
        $flags += "--remap-path-prefix=$tSlash=/t"
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join ([char]0x1F)

    cargo build --release -p ferric-desktop --locked
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
} finally {
    Pop-Location
    & subst $substDrive /D | Out-Null
    if ($cargoDrive) {
        & subst $cargoDrive /D | Out-Null
    }
    $env:MAKEPAD_PACKAGE_DIR = $prevMakepadDir
    $env:CFLAGS = $prevCflags
    $env:CXXFLAGS = $prevCxxflags
    $env:CARGO_ENCODED_RUSTFLAGS = $prevRustflags
    $env:CARGO_TARGET_DIR = $prevTargetDir
    $env:CARGO_HOME = $prevCargoHome
}
