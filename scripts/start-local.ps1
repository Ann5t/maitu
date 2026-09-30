[CmdletBinding()]
param(
    [ValidateRange(1024, 65535)]
    [int]$Port = 3033,
    [string]$ProxyUrl,
    [string]$BuildProxyUrl,
    [switch]$NoBuild,
    [switch]$OpenBrowser
)

$ErrorActionPreference = 'Stop'
$taskRepoRoot = Split-Path -Parent $PSScriptRoot
$taskComposeFile = Join-Path $taskRepoRoot 'compose.maitu.yaml'
$taskPreviousPort = $env:MAITU_PORT
$taskPreviousHttpProxy = $env:HTTP_PROXY
$taskPreviousHttpsProxy = $env:HTTPS_PROXY
$taskPreviousBuildProxy = $env:MAITU_BUILD_PROXY

try {
    $env:MAITU_PORT = [string]$Port
    if (-not $ProxyUrl -and -not $env:HTTPS_PROXY) {
        $taskSystemProxy = Get-ItemProperty -LiteralPath 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Internet Settings' -ErrorAction SilentlyContinue
        if ($taskSystemProxy.ProxyEnable -eq 1 -and $taskSystemProxy.ProxyServer) {
            $taskProxyServer = [string]$taskSystemProxy.ProxyServer
            if ($taskProxyServer -match '=') {
                $taskProxyMatches = [regex]::Match($taskProxyServer, '(?:^|;)https=([^;]+)')
                if (-not $taskProxyMatches.Success) {
                    $taskProxyMatches = [regex]::Match($taskProxyServer, '(?:^|;)http=([^;]+)')
                }
                if ($taskProxyMatches.Success) {
                    $taskProxyServer = $taskProxyMatches.Groups[1].Value
                } else {
                    $taskProxyServer = ''
                }
            }
            if ($taskProxyServer) {
                $ProxyUrl = if ($taskProxyServer -match '^https?://') { $taskProxyServer } else { "http://$taskProxyServer" }
            }
        }
    }
    if ($ProxyUrl) {
        $taskProxyUri = [uri]$ProxyUrl
        if (-not $taskProxyUri.IsAbsoluteUri -or $taskProxyUri.Scheme -notin @('http', 'https')) {
            throw '代理地址须为完整的 HTTP 或 HTTPS URL。'
        }
        $env:HTTP_PROXY = $ProxyUrl
        $env:HTTPS_PROXY = $ProxyUrl
        Write-Host '使用已配置代理下载 Docker 镜像。'
    }
    & docker info --format '{{.ServerVersion}}'
    if ($LASTEXITCODE -ne 0) {
        throw 'Docker 后台尚未就绪，请先启动 Docker Desktop。'
    }
    if ($BuildProxyUrl) {
        $env:MAITU_BUILD_PROXY = $BuildProxyUrl
    } elseif (-not $env:MAITU_BUILD_PROXY -and $env:HTTPS_PROXY) {
        $taskDockerOs = & docker info --format '{{.OperatingSystem}}'
        if ($LASTEXITCODE -eq 0 -and $taskDockerOs -match 'Docker Desktop') {
            $env:MAITU_BUILD_PROXY = 'http://http.docker.internal:3128'
        }
    }
    & docker compose -f $taskComposeFile config --quiet
    if ($LASTEXITCODE -ne 0) {
        throw '脉图启动配置校验失败。'
    }
    $taskComposeArgs = @('compose', '-f', $taskComposeFile, 'up', '-d', '--wait', '--wait-timeout', '180')
    if ($NoBuild) {
        $taskComposeArgs += '--no-build'
    } else {
        $taskComposeArgs += '--build'
    }
    & docker @taskComposeArgs
    if ($LASTEXITCODE -ne 0) {
        throw '脉图未成功启动，请查看镜像下载、构建或服务日志。'
    }
    $taskUrl = "http://localhost:$Port"
    $taskHealth = Invoke-RestMethod -Uri "$taskUrl/api/health" -TimeoutSec 10
    if ($taskHealth.ok -ne $true) {
        throw '服务健康检查未通过。'
    }
    Write-Host "脉图已启动：$taskUrl"
    if ($OpenBrowser) {
        Start-Process -FilePath $taskUrl
    }
} finally {
    $env:MAITU_PORT = $taskPreviousPort
    $env:HTTP_PROXY = $taskPreviousHttpProxy
    $env:HTTPS_PROXY = $taskPreviousHttpsProxy
    $env:MAITU_BUILD_PROXY = $taskPreviousBuildProxy
}
