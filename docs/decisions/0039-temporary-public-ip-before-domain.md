# 0039：公网 IP 仅作临时验收，正式入口切换域名

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

Fudian 需要先让电脑、手机和平板从真实公网完成一轮测试，但测试开始时可能还没有域名。直接使用 `http://公网 IP` 不仅暴露登录和项目内容，也不能可靠使用浏览器麦克风、Service Worker 等安全上下文能力。

Let’s Encrypt 已支持 IPv4 和 IPv6 地址证书，但 IP 证书必须使用短期配置，有效期只有 160 小时。现有公网部署模板只面向域名，不能假定 Caddy 已经自动处理 IP 证书的申请、安装与续期。

## 备选方案

- 等域名准备好以后才做任何真实设备公网测试。
- 临时通过未加密 HTTP 或忽略证书错误访问公网 IP。
- 使用可信的短期 IP 证书完成临时验收，随后把正式入口切换到域名。

## 决定

公网 IP 是上线前的临时验收入口，不是 Fudian 的长期正式地址。测试服务器必须使用稳定、可从公网验证的 IPv4 或 IPv6；只向公网开放 `80/443`。Caddy 是唯一入口，应用、PostgreSQL、项目 Git/Git LFS、Runner、Tool Broker 和插件 Worker 均只在内部网络中访问。

IP 测试入口固定使用 `https://<公网 IP>`。端口 80 只承载 ACME HTTP 验证和跳转，业务流量走 443。不得以裸 HTTP、浏览器忽略证书错误或在各设备安装临时自签根证书代替公网验收。

IP 证书由支持地址证书的 Certbot 5.4 或更高版本通过 `webroot` 与 Let’s Encrypt `shortlived` profile 申请。证书和私钥通过受限持久卷提供给 Caddy，不进入镜像、Git、聊天、日志或普通项目导出。系统至少每 12 小时检查一次续期；成功后通过 deploy hook 原子重载 Caddy。证书剩余不足 48 小时且续期仍失败时，产生实例级待处理状态并按邮件规则通知管理员。

公网 IP 变化会使地址和证书同时失效。第一版不尝试猜测新地址或继续提供不可信入口，而是停止旧入口并要求管理员重新验证和签发。

真实设备、关键交互和恢复演练通过后，管理员配置正式域名、DNS 与精确 `FUDIAN_PUBLIC_ORIGIN`，再切换到 Caddy 的域名 ACME。正式使用只公布域名；临时 IP 入口在短暂切换验证后关闭，不承担永久重定向或兼容旧浏览器会话的义务。

## 影响

- 没有域名时也能测试手机麦克风、PWA、安全 Cookie 和跨设备恢复的真实 HTTPS 行为。
- 短期 IP 证书要求续期完全自动化，并需要到期监控和外部告警。
- 当前公网模板需要增加明确的 IP 验收模式；在实现和验收前，本文不能被当作已经可运行的部署命令。
- 域名切换后用户可能需要重新登录，旧 IP 书签不再作为正式入口维护。

## 相关资料

- [Leptos Web 工作台配合可补齐事件流](0038-leptos-web-workbench-and-replayable-streams.md)
- [可执行邮件只发送需要处理的通知](0040-actionable-email-notifications.md)
- [Let’s Encrypt：短期证书与 IP 地址证书正式可用](https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability.html)
- [Let’s Encrypt：Certbot 申请 IP 地址证书](https://letsencrypt.org/2026/03/11/shorter-certs-certbot)
- [MDN：getUserMedia 仅限安全上下文](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia)
- [产品设计：部署与访问](../product/product-design.md#17-部署与访问)
