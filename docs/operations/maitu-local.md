# 脉图本机运行

本页说明 Maitu 的 Windows 本机运行入口。Docker Desktop 使用 Linux 引擎，应用在本机环回端口提供网页，数据库与项目文件放在 Maitu 专用数据卷。

## Windows 启动

在仓库根目录打开 PowerShell，执行：

```powershell
./scripts/start-local.ps1
```

默认地址为 `http://localhost:3033`。脚本等待容器就绪，并通过应用健康接口检查数据库可连接。首次启动需要下载基础镜像、安装构建依赖和编译 Rust；后续构建会使用本机缓存。

需要更换端口或启动后打开浏览器时：

```powershell
./scripts/start-local.ps1 -Port 3034 -OpenBrowser
```

## Clash 与代理

脚本优先保留终端已有的代理变量。未设置时读取 Windows 已启用的静态系统代理，为 Docker 构建工具的镜像认证请求设置本次进程代理；结束时恢复原环境。

可以显式指定代理，例如 Clash 的 HTTP 混合端口：

```powershell
./scripts/start-local.ps1 -ProxyUrl http://127.0.0.1:7897
```

Docker Desktop 自身也需要通过其系统代理或手动代理访问镜像仓库。端口以 Clash 实际配置为准。构建容器中的 `127.0.0.1` 指向容器自己，因此脚本在 Docker Desktop 上通过其内部 HTTP 代理处理构建依赖下载。其他环境可以用 `-BuildProxyUrl` 指定容器能访问的代理，或自行设置 `MAITU_BUILD_PROXY`。

代理地址不写入 Git、本机全局配置或运行时应用环境。此启动脚本仅设置当前进程与镜像构建参数。

请求进入 Clash 后仍可能匹配到直连。首次完整测试会下载微软的 Playwright 镜像；如果日志显示 `mcr.microsoft.com` 或其 `data.mcr.microsoft.com` 子域走 `DIRECT` 且下载缓慢，可在 Clash 中把 `mcr.microsoft.com` 的域名后缀规则放在微软直连规则之前，选择可用代理组。此规则包括该镜像站的数据子域，不需要把所有微软请求改成代理。本次验收临时为 Docker 的这些请求设置了代理路径，镜像下载后恢复原规则；启动脚本不修改 Clash 规则。

## 服务与数据

```powershell
docker compose -f compose.maitu.yaml ps
docker compose -f compose.maitu.yaml logs --tail 100 app
docker compose -f compose.maitu.yaml stop
```

`compose.maitu.yaml` 默认项目名为 `maitu`，使用项目专属的 PostgreSQL、成果、仓库、工作区和执行输出卷；旧 `compose.yaml` 是继承来源。可以在启动前设置 `MAITU_INSTANCE` 运行另一套独立实例，并选择空闲端口。

应用的本机入口关闭账号认证，并只发布 `127.0.0.1` 上的网页端口。迁到远程服务器时采用[安全部署指南](deployment.md)中的独立配置。

当前已完成与尚待验证的行为见[实施进度](../development/maitu-progress.md)。
