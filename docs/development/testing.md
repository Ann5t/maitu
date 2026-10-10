# 测试指南

> 状态：当前操作指南。

## 日常检查

```bash
make check
```

该命令运行 Markdown 链接检查、rustfmt、Clippy（警告视为错误）和全部 Rust 测试。文档检查也可以单独运行：

```bash
./scripts/check-docs.py
```

## Windows 上的检查环境

`make check`、`./scripts/quality-gate.sh` 和 `scripts/` 下的专项检查都是 Bash，在 Windows 上须通过 WSL 或 Git Bash 执行。仓库用 `.gitattributes` 固定文本文件为 LF；在此之前的旧检出会把脚本写成 CRLF，shebang 变成 `#!/usr/bin/env bash\r`，执行时报 `env: $'bash\r': No such file or directory`。

行尾已经写坏时，重新取出脚本即可恢复（会覆盖 `scripts/` 下未提交的改动）：

```bash
rm -f scripts/*.sh && git checkout -- scripts
```

只执行 `git checkout -- scripts` 不起作用：行尾差异被 Git 的行尾转换掩盖，Git 认为文件没有修改，因而不会重写它们。

## 完整质量门

```bash
./scripts/quality-gate.sh
```

完整入口依次检查：

1. Markdown 结构和相对链接；
2. 认证页标题文案与浏览器 `h1` 断言的一致性；
3. Rust 格式、Clippy 和测试；
4. 空库、增量迁移和旧数据 fixture；
5. 目标、上下文、工作区、调度、审核、工具和输入 HTTP 闭环；
6. 桌面、平板、手机及大图 Chromium 测试；
7. 非 root 生产镜像、安全 HTTPS、存储调和和备份恢复；
8. 工作树空白错误检查。

质量门使用唯一命名的一次性容器、网络和数据库，不连接 Compose 正在使用的正式卷。浏览器截图默认写入临时目录；需要更新文档视觉证据时显式设置 `SCREENSHOT_DIR=docs/assets/screenshots`，并人工审查差异。

专项脚本及其覆盖范围见[脚本索引](../../scripts/README.md)。

## 脉图资料任务

完成依赖下载与 Rust 检查后运行 `./scripts/test-maitu-workflow.sh`，使用隔离 PostgreSQL 与本机 HTTP 测试接口验证请求并行、失败隔离、显式重试、版本引用、并发调整、中断恢复及大量依赖等待时的可执行任务。该检查已加入完整质量门，但不能证明 DeepSeek 账户已接通。

`tests/browser/maitu.spec.js` 使用真实网页表单和数据库，验证资料导入、创建与启动、等待说明、失败与历史、刷新和移动布局。它用本机关闭端口验证网络失败，不调用付费服务。真实 DeepSeek 验收单独保存在[实施进度](maitu-progress.md)。

## 脉图目标与编码

运行 `./scripts/test-maitu-code-workflow.sh`，用隔离数据库、模型协议测试接口及真实 Docker 检查容器验证计划修改采用、两个编码任务同时运行、检查失败后修正、固定版本整合、采用冲突、重复请求与进程中断。该脚本已加入完整质量门。模型协议测试接口不调用付费服务；文件修改、Git 合并和 Node 检查为真实操作。

检查用例还验证模型思考消息连续传递、工具结果回流、补充要求形成新尝试、工作区与检查环境隔离。重复同一检查编号必须返回原回执；不能再次执行命令。中断后保留现场与操作状态，不自动重复外部请求。

网页用例覆盖文件夹导入及忽略项、编码类型与验收字段、编辑并采用计划、未启动节点、历史查看、补充要求与重试，以及桌面和手机上卡片不重叠。计划建议 fixture 明确标为网页测试数据，不作为真实 DeepSeek 证据。真实账户验收另记录模型请求时段、具体代码差异、真实进程结果和采用版本。

完整质量门的 `test-backup-recovery.sh` 还导入一个只有 Maitu 托管代码仓库的实例，实际备份到空目标并比较完整项目信息与代码导出。此用例不创建旧 `projects/*.git`，避免旧路径的有效仓库掩盖新版 `maitu-code/*/repository.git` 没有被校验的问题。

## 隔离集成测试的排错要点

`src/maitu/integration.rs` 里的用例都标了 `#[ignore]`，只在隔离 PostgreSQL 下运行。手写 `docker run ... cargo test` 复跑单个用例时，下面四点都实际造成过**看起来像产品缺陷的假结论**：

1. **`--exact` 只接受完整测试路径，并且要核对真的跑了 1 个用例。**
   短名配 `--exact` 匹配不到任何用例，cargo 仍会打印 `test result: ok. 0 passed; ... N filtered out`；只看 `ok` 会把空跑当成通过。用完整路径，并确认汇总行是 `1 passed`：

   ```bash
   docker run --rm --network "$NET" -w /app -e DATABASE_URL="$URL" "$IMG" \
     cargo test --offline --locked --bin fudian \
     maitu::integration::queued_task_held_for_a_connection_is_not_claimed_by_it \
     -- --ignored --exact --nocapture
   ```

   `scripts/test-maitu-workflow.sh` 不传 `--exact`，逐个执行完整路径，不受这一条影响。

2. **不要在另一个工作树里对共享 target 卷执行 `cargo clean`。**
   `cargo clean -p fudian` 清掉的是卷里的产物，而另一个工作树的指纹仍显得是新的，于是那个工作树会继续运行**用被改过的源码构建出来的二进制**。实测后果是一个质量门全绿的用例在本地连续失败，并报出只有被改过的源码才可能产生的错误。给第二个工作树独立 target 卷，或只在当前工作树清理：

   ```bash
   docker run --rm -w /app --mount "type=bind,src=$OTHER_WORKTREE,dst=/app" \
     --mount "type=volume,src=maitu_alt_target,dst=/app/target" "$IMG" cargo test ...
   ```

3. **`EXTRACT(EPOCH FROM ...)` 返回 `numeric`，sqlx 不能把它解码成 `f64`。**
   取持锁剩余时间这类数值要显式转型，否则 `unwrap()` 报解码错误，掩盖真正要看的断言：

   ```sql
   SELECT (EXTRACT(EPOCH FROM (hold_until - now())))::float8 FROM maitu_tasks WHERE id=$1
   ```

4. **Windows 工作树是 CRLF、Git 存 LF，脚本类文件尤其要确认差异行数。**
   在 Windows 侧改 `scripts/*.sh` 后用 WSL 的 git `add`，会把整个脚本按 CRLF 记进提交（diff 显示上百行），而 CRLF 的 bash 脚本在 Linux 上无法执行。提交脚本类文件用 Windows 的 git（`core.autocrlf=true` 会归一为 LF），并用 `git diff --stat HEAD^ HEAD` 确认它只改了实际修改的行数。

与用例本身有关的一点：`fail_or_retry` 会在**新的 attempt 行**里为重试排队，所以一次失败之后任务本来就有两行，判定要按尝试状态计数，不能把 `attempts.len()` 当成"尝试次数"。
