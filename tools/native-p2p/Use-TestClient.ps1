param(
    [ValidateSet('Prepare','Restore')][string]$Mode = 'Prepare',
    [string]$Package,
    [string]$UserProfile = $env:USERPROFILE,
    [string]$BackupDirectory,
    [string]$Server,
    [string]$ServerKey
)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an administrator PowerShell. The RustDesk service and its configuration require elevation.'
}

function Stop-Clients([string[]]$Executables) {
    Stop-Service -Name RustDesk -ErrorAction Stop
    (Get-Service RustDesk).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(20))
    Get-CimInstance Win32_Process -Filter "Name = 'rustdesk.exe'" | Where-Object {
        $_.ExecutablePath -and $Executables -contains $_.ExecutablePath
    } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Set-Options([string]$Path) {
    $text = if (Test-Path -LiteralPath $Path) { [IO.File]::ReadAllText($Path) } else { '' }
    $match = [regex]::Match($text, '(?ms)^\[options\]\r?\n(?<body>.*?)(?=^\[|\z)')
    $body = $match.Groups['body'].Value
    foreach ($pair in @{
        'custom-rendezvous-server' = $Server
        'relay-server' = ($Server -replace ':\d+$', ':22117')
        'key' = $ServerKey
        'enable-native-p2p' = 'Y'
    }.GetEnumerator()) {
        $key = [regex]::Escape($pair.Key)
        $pattern = '(?m)^(?:"' + $key + '"|' + $key + ')\s*=.*$'
        $line = $pair.Key + " = '" + $pair.Value + "'"
        if ([regex]::IsMatch($body, $pattern)) { $body = [regex]::Replace($body, $pattern, $line) }
        else { $body += "`r`n$line`r`n" }
    }
    $section = "[options]`r`n$body"
    if ($match.Success) { $text = $text.Remove($match.Index, $match.Length).Insert($match.Index, $section) }
    else { $text += "`r`n$section" }
    [IO.File]::WriteAllText($Path, $text, [Text.UTF8Encoding]::new($false))
}

if ($Mode -eq 'Restore') {
    if (!$BackupDirectory) { throw 'Restore requires -BackupDirectory from the Prepare result.' }
    $state = Get-Content -LiteralPath (Join-Path $BackupDirectory 'state.json') -Raw | ConvertFrom-Json
    Stop-Clients @($state.original_exe, $state.test_exe)
    foreach ($entry in $state.files) {
        if ($entry.existed) { Copy-Item -LiteralPath $entry.backup -Destination $entry.path -Force }
        elseif (Test-Path -LiteralPath $entry.path) { Remove-Item -LiteralPath $entry.path -Force }
    }
    & sc.exe config RustDesk binPath= $state.service_path | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Unable to restore service executable' }
    Start-Service RustDesk
    [pscustomobject]@{ restored=$true; service_path=$state.service_path; backup=$BackupDirectory } | ConvertTo-Json
    exit
}

if (!$Package -or !(Test-Path -LiteralPath $Package -PathType Leaf)) { throw 'Prepare requires the Windows test ZIP in -Package.' }
if ($Server -notmatch '^[a-zA-Z0-9.-]+:\d+$' -or $ServerKey -notmatch '^[a-zA-Z0-9+/=]+$') { throw 'Invalid test server or public key' }
if (!(Test-Path -LiteralPath (Join-Path $UserProfile 'AppData\Roaming') -PathType Container)) { throw 'Invalid -UserProfile' }
$service = Get-CimInstance Win32_Service -Filter "Name = 'RustDesk'"
if (!$service -or $service.PathName -notmatch '^"([^"]+)"\s+--service$') { throw 'An existing standard RustDesk service is required' }
$original = $Matches[1]
if ($original -match '\\RustDeskNativeP2PTest\\') { throw 'Restore the previous test before preparing another one' }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (!$BackupDirectory) { $BackupDirectory = Join-Path $UserProfile "AppData\Local\RustDeskNativeP2PTest\backup-$stamp" }
if (Test-Path -LiteralPath (Join-Path $BackupDirectory 'state.json')) { throw 'Refusing to overwrite a previous backup' }
$install = Join-Path $env:ProgramFiles "RustDeskNativeP2PTest\$stamp"
New-Item -ItemType Directory -Path $install,$BackupDirectory -Force | Out-Null
Expand-Archive -LiteralPath $Package -DestinationPath $install
$testExe = Join-Path $install 'rustdesk.exe'
if (!(Test-Path -LiteralPath $testExe) -or !(Test-Path -LiteralPath (Join-Path $install 'librustdesk.dll'))) { throw 'Incomplete test package' }
$directories = @(
    (Join-Path $UserProfile 'AppData\Roaming\RustDesk\config'),
    (Join-Path $env:WINDIR 'ServiceProfiles\LocalService\AppData\Roaming\RustDesk\config')
)
Stop-Clients @($original, $testExe)
$files = @()
foreach ($directory in $directories) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    foreach ($name in @('RustDesk.toml', 'RustDesk2.toml')) {
        $path = Join-Path $directory $name
        $backup = Join-Path $BackupDirectory ($files.Count.ToString() + '-' + $name)
        $existed = Test-Path -LiteralPath $path
        if ($existed) { Copy-Item -LiteralPath $path -Destination $backup }
        $files += [pscustomobject]@{path=$path;backup=$backup;existed=$existed}
    }
}
$state = [pscustomobject]@{ service_path=$service.PathName;original_exe=$original;test_exe=$testExe;files=$files;started=$stamp }
$state | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $BackupDirectory 'state.json') -Encoding UTF8
foreach ($directory in $directories) { Set-Options (Join-Path $directory 'RustDesk2.toml') }
& sc.exe config RustDesk binPath= ('"' + $testExe + '" --service') | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Unable to set test service; restore from $BackupDirectory" }
Start-Service RustDesk
(Get-Service RustDesk).WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
[pscustomobject]@{ prepared=$true;test_exe=$testExe;backup=$BackupDirectory;server=$Server;service_running=$true } | ConvertTo-Json
