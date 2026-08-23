# 0026：工具能力由环境、适配器和 Agent 说明组合

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

把 Rust、Python、GCC、Playwright、PPTMaster 和未来所有工具都叫作插件，容易误解为每项能力都要携带一份完整容器和私有开发环境。这样会破坏已经确定的版本去重，也让常见命令的包装和启动成本过高。

反过来，把所有版本和程序安装进一个可任意修改的大工具容器，又会让升级、依赖冲突、权限和 Session 隔离失去边界。现有实现偏向自定义 PluginManifest 加签名 OCI Worker，MCP 传输仍未固定，也不足以单独代表最终插件接口。

## 备选方案

- 每个工具能力都必须携带完整独立容器。
- 所有工具共享一个可变的大容器和全局依赖环境。
- 统一目录对外，内部把环境资源、调用适配器和 Agent 说明作为可组合部分。

## 决定

用户和 Agent 只面对一个统一工具目录。目录中的一项能力由以下部分按需组合，而不是要求每项全部自带：

- 共享环境资源：准确版本的编译器、运行时、浏览器和依赖内容；
- 调用适配器：内置受控命令执行、浏览器执行、MCP 服务或专用 Runtime 入口；
- Agent 使用说明：简短能力描述、可选 Skill、references、模板和示例。

常规 Rust、C/C++、Python、Node 开发使用内置开发执行器和中央环境资源。Playwright/Chromium 使用受控浏览器执行器。PPTMaster、Blender、外部服务等特殊能力可以提供专用适配器、Skill 和必要 Runtime。目录中的 Rust Tool Pack 可以引用共享 Rust 资源，但不包含一份无法与其他能力复用的私有安装。

MCP、MCPB、OCI 和以后增加的运行格式只作为 Tool Broker 的兼容协议、导入格式或执行载体。它们统一转换为 Fudian 的能力、权限、环境、输入、输出、文件变更和 Evidence 合同，不能各自绕过 worktree 隔离或创造另一套授权语义。

## 影响

- 常用工具调用保持轻量，特殊插件仍能携带复杂能力。
- 统一界面不要求底层只有一个大容器，也不会暴露多套权限模型。
- 现有 MCP 工具可以通过适配器接入，Fudian 不需要重新发明其调用 schema；版本固定、运行隔离和文件写回仍由 Fudian 负责。
- 后续仍需确定添加一个新能力时的最小描述文件、适配器类型和开发/发布流程。

## 相关资料

- [统一工具目录复用不可变资源并隔离 Session 环境](0024-unified-tools-with-isolated-reused-environments.md)
- [管理员安装插件且 Session 固定工具版本](0025-admin-plugin-install-and-pinned-session-tools.md)
- [产品设计：插件构成](../product/product-design.md#111-插件构成)
