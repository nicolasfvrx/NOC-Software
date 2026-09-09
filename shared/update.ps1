# Compatible with Windows PowerShell 3 / Windows Server 2012 and .NET 4.5.
$ErrorActionPreference = 'Stop'
$app = $env:NOC_UPDATE_APP
$target = $env:NOC_UPDATE_EXE
$stage = $env:NOC_UPDATE_STAGE
$updates = Split-Path -Parent $stage
$lock = $null
$handedOff = $false
$backup = Join-Path $updates ($app + '.previous')

function Version-Of([string]$value) {
    if ($value -notmatch '^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') { return $null }
    return [version]($value.TrimStart('v'))
}

function Download([string]$url, [string]$path, [long]$limit, [bool]$binary) {
    for ($redirect = 0; $redirect -lt 6; $redirect++) {
        $uri = [Uri]$url
        if ($uri.Scheme -ne 'https') { throw 'HTTPS required' }
        $request = [Net.HttpWebRequest]::Create($uri)
        $request.UserAgent = 'NOC-Startup-Updater'
        $request.Accept = if ($binary) { 'application/octet-stream' } else { 'application/vnd.github+json' }
        $request.Timeout = 12000
        $request.ReadWriteTimeout = 12000
        $request.AllowAutoRedirect = $false
        if ($redirect -eq 0 -and $env:NOC_GITHUB_TOKEN) {
            $request.Headers['Authorization'] = 'Bearer ' + $env:NOC_GITHUB_TOKEN
        }
        $response = $request.GetResponse()
        try {
            if ([int]$response.StatusCode -ge 300 -and [int]$response.StatusCode -lt 400) {
                $url = (New-Object Uri($uri, $response.Headers['Location'])).AbsoluteUri
                continue
            }
            if ([int]$response.StatusCode -ne 200) { throw 'Unexpected HTTP status' }
            $inputStream = $response.GetResponseStream()
            $outputStream = [IO.File]::Create($path)
            try {
                $buffer = New-Object byte[] 65536
                [long]$total = 0
                while (($count = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    $total += $count
                    if ($total -gt $limit) { throw 'Download too large' }
                    $outputStream.Write($buffer, 0, $count)
                }
            } finally { $outputStream.Dispose(); $inputStream.Dispose() }
            return
        } finally { $response.Dispose() }
    }
    throw 'Too many redirects'
}

function Hash-Of([string]$path) {
    $stream = [IO.File]::OpenRead($path)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
}

function Quote-Argument([string]$arg) {
    $escaped = [regex]::Replace($arg, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Relaunch {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $target
    $info.WorkingDirectory = $env:NOC_UPDATE_CWD
    $info.UseShellExecute = $false
    $info.EnvironmentVariables['NOC_SKIP_UPDATE'] = $app
    $arguments = @()
    if ($env:NOC_UPDATE_ARGS) {
        foreach ($encoded in $env:NOC_UPDATE_ARGS.Split("`n")) {
            $hex = $encoded.Substring(1)
            $bytes = New-Object byte[] ($hex.Length / 2)
            for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [Convert]::ToByte($hex.Substring($i * 2, 2), 16) }
            $arguments += Quote-Argument ([Text.Encoding]::Unicode.GetString($bytes))
        }
    }
    $info.Arguments = $arguments -join ' '
    if ($app -eq 'noc-display') { $info.CreateNoWindow = $true }
    $process = [Diagnostics.Process]::Start($info)
    $process.Dispose()
}

try {
    # Exclusive file handle is released by Windows even if a helper is killed.
    $lock = [IO.File]::Open((Join-Path $updates ($app + '.lock')), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $initialHash = Hash-Of $target
    $current = Version-Of $env:NOC_UPDATE_CURRENT
    if ($null -eq $current) { return }
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $api = 'https://api.github.com/repos/nicolasfvrx/NOC-Software'
    $metadataFile = Join-Path $stage 'release.json'
    Download ($api + '/releases/latest') $metadataFile 2097152 $false
    $release = [IO.File]::ReadAllText($metadataFile) | ConvertFrom-Json
    $remote = Version-Of $release.tag_name
    if ($release.draft -or $release.prerelease -or $null -eq $remote -or $remote -le $current) { return }
    $matches = @($release.assets | Where-Object { $_.name -eq $env:NOC_UPDATE_ASSET -and $_.state -eq 'uploaded' })
    if ($matches.Count -ne 1) { throw 'No matching release asset' }
    $asset = $matches[0]
    if ([string]$asset.id -notmatch '^\d+$' -or $asset.size -le 0 -or $asset.size -gt 134217728) { throw 'Invalid asset' }
    $candidate = Join-Path $stage ($app + '.exe')
    Download ($api + '/releases/assets/' + $asset.id) $candidate 134217728 $true
    if ((Get-Item -LiteralPath $candidate).Length -ne $asset.size -or ('sha256:' + (Hash-Of $candidate)) -cne $asset.digest) { throw 'Executable checksum mismatch or unavailable' }
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($candidate)
    $expectedName = if ($app -eq 'noc-manager') { 'NOC Manager' } else { 'NOC Display' }
    if ($version.ProductName -cne $expectedName) { throw 'Unexpected executable identity' }
    # ReplaceFile preserves the installed file's ACL; no inherited access widening.
    $parent = [Diagnostics.Process]::GetProcessById([int]$env:NOC_UPDATE_PARENT)
    [IO.File]::WriteAllText((Join-Path $stage 'ready'), 'yes')
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path -LiteralPath (Join-Path $stage 'commit'))) {
        if ([DateTime]::UtcNow -gt $deadline) { return }
        Start-Sleep -Milliseconds 100
    }
    if (-not $parent.WaitForExit(15000)) { return }
    $handedOff = $true
    if ((Hash-Of $target) -cne $initialHash) { throw 'Executable changed during update' }
    [IO.File]::Replace($candidate, $target, $backup, $false)
    try { Relaunch }
    catch {
        [IO.File]::Replace($backup, $target, $null, $false)
        Relaunch
    }
    $handedOff = $false
} catch {
    try { [IO.File]::AppendAllText((Join-Path $updates ($app + '.log')), ('Update skipped: ' + $_.Exception.GetType().Name + "`r`n")) } catch { }
    # In-use executable (another session), download or replacement error: keep the old app.
    if ($handedOff) { try { Relaunch } catch { } }
} finally {
    if ($null -ne $lock) { $lock.Dispose() }
    # Only this helper's generated staging directory is removed.
    if ($handedOff -or (Test-Path -LiteralPath (Join-Path $stage 'commit'))) {
        $resolvedStage = [IO.Path]::GetFullPath($stage)
        $resolvedUpdates = [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $target) '.noc-updates'))
        if ($resolvedStage.StartsWith($resolvedUpdates + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedStage -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
