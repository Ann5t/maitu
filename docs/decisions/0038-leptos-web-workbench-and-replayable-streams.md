# 0038：Leptos Web 工作台配合可补齐事件流

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

当前 Maud 服务端 HTML 与少量 JavaScript 已能支持表单和静态投影，但目标产品需要无限画布、细粒度实时状态、自动跟随现场、断线补齐、编辑器、终端和移动端手势。继续向全页服务端渲染追加零散脚本会形成两套状态来源，也难以测试复杂交互。

另一方面，代码编辑器、终端和图布局已有成熟浏览器组件。为了追求字面上的“零 JavaScript”重新实现这些底层能力，会增加风险而不改善 Fudian 的 Rust 领域核心。

## 备选方案

- 保留 Maud 全页渲染并继续追加原生脚本。
- 使用 Dioxus 并同时发布 Web、桌面和原生手机程序。
- 使用 Leptos 构建 Web/PWA，保留 Axum 后端并封装少量成熟浏览器组件。

## 决定

第一版只发布响应式 Web/PWA。后端继续使用 Rust、Axum、Tokio、SQLx 和明确的领域/API 边界；交互前端使用 Rust、Leptos 与 WebAssembly。首屏由服务器渲染并在浏览器 hydration，随后由客户端 Rust 状态驱动细粒度更新。现有 Maud 页面是迁移来源，不成为第二套长期 UI 框架。

业务命令继续通过带身份、版本、幂等键和审计的显式 Axum API，不因为 Leptos server functions 而绕过已确定合同。服务器与浏览器共享序列化类型和稳定错误码，但数据库模型不直接暴露给客户端。

少量浏览器专用能力通过类型化适配层固定版本使用：

- 目标画布由 Leptos/Rust 管理状态并渲染 SVG；ELK.js 在 Web Worker 中只计算初始或局部自动布局，不拥有目标图事实；
- CodeMirror 6 提供桌面和移动浏览器中的文本编辑、只读查看及 diff/merge；
- xterm.js 提供终端显示，默认以观察 Agent 输出为主；真正交互式 shell 仍受独立 ToolLease 和权限约束；
- CSS、Web Animations API 和 View Transition API 处理普通动效，不先引入通用动画运行时。

权威实时更新使用服务器发送事件（SSE）：每个可恢复事件带单调位置，客户端保存最后位置；重连先获取缺失事件或必要快照，再继续订阅。用户消息、暂停、Proposal 和 MergeGate 决定继续走普通 HTTPS 命令。只有交互式终端、远程浏览器控制等确实需要低延迟双向字节流的短期现场才建立作用域明确的 WebSocket，不能用一个全局 WebSocket 承担所有领域状态。

Rust 依赖由 `Cargo.lock` 固定；CodeMirror、xterm.js、ELK.js 和构建工具由前端锁文件固定并在 CI 构建。生产镜像携带已构建、带内容摘要的静态资源，运行时不从公共网络下载前端依赖。

## 影响

- 产品逻辑和客户端状态仍主要使用 Rust，同时复用成熟编辑器、终端和布局算法。
- 断线恢复建立在持久事件位置上，不依赖某个 WebSocket 连接一直存在。
- 第一版不维护桌面、Android 和 iOS 原生工程；以后可以在真实系统级需求出现后增加外壳。
- 构建链会增加 WebAssembly 和前端包锁文件，但运行部署仍是同一套 Fudian Web 服务与静态资源。
- 当前 Maud 视图和零散 `app.js` 需要在正式测试基线前迁移并删除重复实现。

## 相关资料

- [Session 工作现场按 Agent 当前动作自动跟随](0037-action-following-session-worksite.md)
- [Leptos 服务端渲染与 Axum](https://book.leptos.dev/ssr/index.html)
- [Monaco Editor 的移动端限制](https://github.com/microsoft/monaco-editor/blob/main/README.md)
- [CodeMirror 参考](https://codemirror.net/docs/ref/)
- [xterm.js 文档](https://xtermjs.org/docs/)
- [ELK 文档](https://eclipse.dev/elk/gettingstarted.html)
- [SSE 事件 ID 与重连](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events)
- [产品设计：部署与访问](../product/product-design.md#17-部署与访问)
