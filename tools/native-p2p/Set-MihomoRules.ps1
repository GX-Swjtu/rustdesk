param(
    [Parameter(Mandatory=$true)][string]$RulesFile,
    [string]$ControllerPipe,
    [string]$ConfigurationDirectory = (Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'io.github.clash-verge-rev.clash-verge-rev')
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Invoke-Core([string]$Method, [string]$Path, [object]$Body) {
    $headers = @{ Authorization = "Bearer $script:apiKey" }
    $json = if ($null -eq $Body) { '' } else { $Body | ConvertTo-Json -Depth 8 -Compress }
    if ($script:controller -match '^127\.0\.0\.1:\d+$') {
        if ($Method -eq 'GET') { return Invoke-RestMethod -Uri "http://$script:controller$Path" -Headers $headers }
        return Invoke-RestMethod -Uri "http://$script:controller$Path" -Method $Method -Headers $headers -Body $json -ContentType 'application/json'
    }
    if (!$script:pipeName) { throw 'No supported local Mihomo controller found' }
    $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $script:pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
    try {
        $pipe.Connect(3000)
        $bodyBytes = [Text.Encoding]::UTF8.GetBytes($json)
        $head = "$Method $Path HTTP/1.1`r`nHost: localhost`r`nAuthorization: Bearer $script:apiKey`r`nContent-Type: application/json`r`nContent-Length: $($bodyBytes.Length)`r`nConnection: close`r`n`r`n"
        $headBytes = [Text.Encoding]::ASCII.GetBytes($head)
        $pipe.Write($headBytes, 0, $headBytes.Length)
        $pipe.Write($bodyBytes, 0, $bodyBytes.Length)
        $memory = [IO.MemoryStream]::new()
        $buffer = New-Object byte[] 8192
        while ($true) {
            $read = $pipe.ReadAsync($buffer, 0, $buffer.Length)
            if (!$read.Wait(5000)) { throw 'Mihomo pipe read timed out' }
            $count = $read.Result
            if ($count -eq 0) { break }
            $memory.Write($buffer, 0, $count)
            if ($memory.Length -gt 2097152) { throw 'Mihomo response too large' }
        }
        $bytes = $memory.ToArray()
        $ascii = [Text.Encoding]::ASCII.GetString($bytes)
        $start = $ascii.IndexOf("`r`n`r`n") + 4
        if ($start -lt 4 -or $ascii -notmatch '^HTTP/1\.[01] 2\d\d') { throw 'Mihomo rejected configuration request' }
        if ($bytes.Length -eq $start) { return $null }
        if ($ascii.Substring(0, $start) -match '(?i)Transfer-Encoding: chunked') {
            $decoded = [IO.MemoryStream]::new()
            while ($true) {
                $end = $ascii.IndexOf("`r`n", $start)
                $size = [Convert]::ToInt32($ascii.Substring($start, $end-$start).Split(';')[0], 16)
                if ($size -eq 0) { break }
                $start = $end + 2
                $decoded.Write($bytes, $start, $size)
                $start += $size + 2
            }
            return [Text.Encoding]::UTF8.GetString($decoded.ToArray()) | ConvertFrom-Json
        }
        return [Text.Encoding]::UTF8.GetString($bytes, $start, $bytes.Length-$start) | ConvertFrom-Json
    } finally { $pipe.Dispose() }
}

$runtimePath = Join-Path $ConfigurationDirectory 'clash-verge.yaml'
$rulesPath = Join-Path (Join-Path $ConfigurationDirectory 'profiles') $RulesFile
$runtime = [IO.File]::ReadAllText($runtimePath)
$saved = [IO.File]::ReadAllText($rulesPath)
function Setting([string]$Name) {
    $match = [regex]::Match($runtime, "(?m)^${Name}:\s*([^\r\n]*)")
    return $match.Groups[1].Value.Trim().Trim('"').Trim("'")
}
$script:apiKey = Setting 'secret'
$script:controller = Setting 'external-controller'
$script:pipeName = (Setting 'external-controller-pipe').Split('\')[-1]
if ($ControllerPipe) { $script:pipeName = $ControllerPipe }
$before = Invoke-Core 'GET' '/configs' $null
if (!$before.tun.enable) { throw 'TUN must already be enabled; this script does not change TUN mode' }
$backup = Join-Path $ConfigurationDirectory ('native-p2p-backups\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backup | Out-Null
Copy-Item -LiteralPath $runtimePath -Destination (Join-Path $backup 'runtime-before.yaml')
Copy-Item -LiteralPath $rulesPath -Destination (Join-Path $backup 'rules-before.yaml')
$rules = @('PROCESS-NAME,RustDesk.exe,DIRECT', 'PROCESS-NAME,rustdesk.exe,DIRECT', 'PROCESS-NAME,native-p2p-probe.exe,DIRECT', 'PROCESS-NAME,native-kcp-probe.exe,DIRECT', 'DOMAIN,stun.easyvoip.com,DIRECT')
foreach ($rule in $rules) {
    if (!$saved.Contains($rule)) {
        if ($saved -match '(?m)^prepend: \[\]') { $saved = $saved -replace '(?m)^prepend: \[\]', "prepend:`n  - $rule" }
        else { $saved = $saved.Replace('prepend:', "prepend:`n  - $rule") }
    }
    if (!$runtime.Contains($rule)) { $runtime = [regex]::Replace($runtime, '(?m)^rules:\r?\n', "rules:`n- $rule`n") }
}
[IO.File]::WriteAllText($rulesPath, $saved, [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($runtimePath, $runtime, [Text.UTF8Encoding]::new($false))
Invoke-Core 'PUT' '/configs?force=true' @{payload=$runtime} | Out-Null
$after = Invoke-Core 'GET' '/configs' $null
$active = Invoke-Core 'GET' '/rules' $null
[pscustomobject]@{backup=$backup; tun_enabled=$after.tun.enable; rules=@($active.rules | Where-Object {$_.payload -match '^(RustDesk.exe|rustdesk.exe|native-p2p-probe.exe|native-kcp-probe.exe|stun.easyvoip.com)$'} | Select-Object type,payload,proxy)} | ConvertTo-Json -Depth 5
