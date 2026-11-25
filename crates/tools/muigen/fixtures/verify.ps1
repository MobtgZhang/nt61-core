param(
    [Parameter(Mandatory = $true)][string]$Muigen,
    [string]$Muirct = "muirct.exe",
    [string]$ProjectFixtures
)

$ErrorActionPreference = "Stop"
$Muigen = (Get-Command $Muigen -CommandType Application).Source
$Muirct = (Get-Command $Muirct -CommandType Application).Source
$fixture = Split-Path -Parent $MyInvocation.MyCommand.Path
$work = Join-Path ([IO.Path]::GetTempPath()) ("muigen test " + [Guid]::NewGuid())
if ($ProjectFixtures) {
    $ProjectFixtures = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ProjectFixtures)
    if (Test-Path -LiteralPath $ProjectFixtures) {
        throw "Choose a new project fixture directory: $ProjectFixtures"
    }
}

function Check-Exit($operation) {
    if ($LASTEXITCODE -ne 0) { throw "$operation exited with code $LASTEXITCODE" }
}

New-Item -ItemType Directory -Path $work | Out-Null
Push-Location $work
try {
    Copy-Item (Join-Path $fixture "sample.rcconfig") "sample resources.rcconfig"
    $sample = Join-Path $fixture "sample.rc"
    & rc.exe /nologo /fo sample.res (Join-Path $fixture "sample.rc")
    Check-Exit "rc.exe"
    & link.exe /nologo /dll /noentry /machine:x64 "/out:My App.dll" sample.res
    Check-Exit "link.exe"
    & cl.exe /nologo /MT /Fe:verify.exe (Join-Path $fixture "verify.c") /link user32.lib
    Check-Exit "cl.exe"
    & $Muigen split --input "My App.dll" --config "sample resources.rcconfig" `
        --lang zh-CN --out "sample distribution" --muirct $Muirct
    Check-Exit "muigen"
    & $Muirct -d "sample distribution\zh-CN\My App.dll.mui"
    Check-Exit "MUI metadata"
    & .\verify.exe (Join-Path $work "sample distribution\My App.dll")
    Check-Exit "resource load from main DLL"
    Rename-Item "sample distribution\zh-CN" "satellite offline"
    & .\verify.exe (Join-Path $work "sample distribution\My App.dll") --missing
    Check-Exit "resource load with satellite absent"

    New-Item -ItemType Directory -Path english | Out-Null
    $config = Get-Content -LiteralPath "sample resources.rcconfig" -Raw
    $config.Replace('language="zh-CN"', 'language="en-US"') |
        Set-Content -LiteralPath "english resources.rcconfig" -Encoding ASCII
    & rc.exe /nologo /d SAMPLE_EN_US /fo english.res $sample
    Check-Exit "rc.exe English"
    & link.exe /nologo /dll /noentry /machine:x64 "/out:english\My App.dll" english.res
    Check-Exit "link.exe English"
    & $Muigen batch --input "My App.dll" --config "sample resources.rcconfig" --lang zh-CN `
        --input "english\My App.dll" --config "english resources.rcconfig" --lang en-US `
        --out "batch distribution" --muirct $Muirct
    Check-Exit "muigen batch"
    & .\verify.exe (Join-Path $work "batch distribution\My App.dll")
    Check-Exit "batch zh-CN resource load"
    & .\verify.exe (Join-Path $work "batch distribution\My App.dll") --en-US
    Check-Exit "batch en-US resource load"

    & $Muigen build --input $sample --config "sample resources.rcconfig" --lang zh-CN `
        --module "My App.dll" --machine x64 --out "built distribution"
    Check-Exit "muigen build"
    & link.exe /nologo /dll /noentry /manifest:no /machine:x64 "/out:built distribution\My App.dll" `
        "built distribution\My App.dll.res"
    Check-Exit "link.exe main DLL"
    & .\verify.exe (Join-Path $work "built distribution\My App.dll")
    Check-Exit "built resources load from main DLL"

    if ($ProjectFixtures) {
        $projectFiles = @{
            "batch-main.pe" = "batch distribution\My App.dll"
            "batch-zh-CN.pe" = "batch distribution\zh-CN\My App.dll.mui"
            "batch-en-US.pe" = "batch distribution\en-US\My App.dll.mui"
            "built-main.pe" = "built distribution\My App.dll"
            "built-zh-CN.pe" = "built distribution\zh-CN\My App.dll.mui"
        }
        New-Item -ItemType Directory -Path $ProjectFixtures | Out-Null
        foreach ($projectFile in $projectFiles.GetEnumerator()) {
            Copy-Item -LiteralPath $projectFile.Value -Destination (Join-Path $ProjectFixtures $projectFile.Key)
        }
    }
} finally {
    Pop-Location
    Remove-Item -LiteralPath $work -Recurse -Force
}
