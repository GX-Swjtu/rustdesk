param([Parameter(Mandatory=$true)][string]$Package)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (!([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run from an administrator PowerShell to update the existing test service.'
}
if (!(Test-Path -LiteralPath $Package -PathType Leaf)) { throw 'Test ZIP not found' }
$service = Get-CimInstance Win32_Service -Filter "Name='RustDesk'"
if (!$service -or $service.PathName -notmatch '^"([^"]+)"\s+--service$') { throw 'Expected an existing RustDesk test service' }
$executable = $Matches[1]
$directory = [IO.Path]::GetFullPath((Split-Path -Parent $executable))
$root = [IO.Path]::GetFullPath((Join-Path $env:ProgramFiles 'RustDeskNativeP2PTest')) + '\'
if (!$directory.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing to update a production installation' }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backup = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) "RustDeskNativeP2PTest\update-$stamp"
$staging = Join-Path $backup 'package'
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Expand-Archive -LiteralPath $Package -DestinationPath $staging
if (!(Test-Path -LiteralPath (Join-Path $staging 'librustdesk.dll')) -or !(Test-Path -LiteralPath (Join-Path $staging 'rustdesk.exe'))) { throw 'Incomplete client package' }
$expected = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $staging 'librustdesk.dll')).Hash
Stop-Service RustDesk
(Get-Service RustDesk).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(20))
Get-CimInstance Win32_Process -Filter "Name='rustdesk.exe'" | Where-Object { $_.ExecutablePath -eq $executable } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
try {
    Compress-Archive -Path (Join-Path $directory '*') -DestinationPath (Join-Path $backup 'previous-client.zip')
    Get-ChildItem -LiteralPath $staging | Copy-Item -Destination $directory -Recurse -Force
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $directory 'librustdesk.dll')).Hash
    if ($actual -ne $expected) { throw 'Updated DLL hash mismatch' }
} finally {
    Start-Service RustDesk
}
(Get-Service RustDesk).WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
[pscustomobject]@{ updated=$true; executable=$executable; dll_sha256=$actual; previous_package=(Join-Path $backup 'previous-client.zip'); service_running=$true } | ConvertTo-Json
