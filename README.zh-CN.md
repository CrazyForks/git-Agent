# Git Agent

[English](README.md) · [简体中文](README.zh-CN.md)

基于 Rust 和 egui 构建的原生桌面 Git 客户端，支持 **Windows、macOS 和 Linux**。
在一个应用中管理仓库、审查改动、浏览历史和解决冲突，并可选用 AI 辅助三方合并。

[下载](https://github.com/adoin/git-Agent/releases/latest) ·
[快速开始](#快速开始) ·
[从源码构建](#从源码构建) ·
[参与贡献](CONTRIBUTING.md) ·
[支持项目](#支持项目) ·
[反馈问题](https://github.com/adoin/git-Agent/issues)

Git Agent 正在持续开发中。发布的安装包与开发分支可能存在差异，使用新功能前请确认当前版本是否支持。

## 功能概览

### 日常 Git 操作

- 打开、克隆和初始化仓库，通过工作空间和仓库标签页组织项目。
- 查看已暂存和未暂存的改动，批量暂存或取消暂存文件，并创建提交。
- 管理分支、标签和储藏（stash），配置远程仓库及分支上游。
- 按需配置获取（fetch）、拉取（pull）和推送（push）选项，为不同仓库设置提交身份。
- 编辑 `.gitignore` 规则，并查看基于 Git 语法的逐行解释，无需调用 AI。

### 历史浏览与改动审查

- 浏览提交图、搜索提交，以及按改动文件查找提交。
- 查看提交详情、文件改动和逐行追溯信息（blame）。
- 比较工作区与指定提交的差异。
- 在独立的双栏差异窗口中查看文件，支持语法高亮。
- 创建和应用补丁，支持为选中的工作区文件或提交生成补丁。

### 分支操作与冲突解决

- 合并分支、拣选（cherry-pick）、反做（revert）和重置（reset）提交。
- 规划交互式变基，支持 pick、reword、edit、squash、fixup 和 drop 操作。
- 继续、跳过或中止进行中的变基。
- 在独立的三方合并编辑器中比较两侧内容、编辑合并结果、跳转冲突，并在保存前撤销或重做修改。
- 请求 AI 合并建议，查看解释和解决方案，自主决定采用哪些改动。详见 [AI 辅助合并](#ai-辅助合并)。

### 仓库工具与个性化

- 配置 Git Flow 的功能分支（feature）、发布分支（release）和热修复分支（hotfix）工作流。
- 添加子模块和子树，配置 Git LFS 跟踪规则并执行 LFS 操作。
- 使用自定义 Git 操作和仓库性能诊断工具。
- 切换简体中文或英文、浅色或深色模式，并调整强调色、界面字体和代码字体。
- 通过[纯数据语言插件](docs/syntax-plugins.md)扩展语法高亮。

## 下载与安装

从 [GitHub 最新版本](https://github.com/adoin/git-Agent/releases/latest)下载适合你系统的安装包。
使用已打包的应用**不需要安装 Rust 或 PowerShell**。

| 平台 | 发布包 | 安装方式 |
| --- | --- | --- |
| Windows x64 | `GitAgentSetup-v<version>.exe` | 运行安装程序，选择安装目录。 |
| macOS，Apple Silicon 和 Intel | `GitAgent-<version>-macOS.dmg` | 打开磁盘映像，将 **Git Agent.app** 拖入 **Applications（应用程序）**。 |
| Linux，Debian/Ubuntu amd64 | `GitAgent_<version>_amd64.deb` | 使用软件安装器打开，或通过 `apt` 安装。 |

macOS 发布包为通用架构版本。Linux 目前提供 Debian 安装包；其他发行版可安装对应的原生开发库后[从源码构建](#从源码构建)。

安装已下载的 Debian 包时，请将下面的文件名替换为实际文件名：

```sh
sudo apt install "./GitAgent_<version>_amd64.deb"
```

### 运行要求

- **必须安装 Git，并确保能通过 `PATH` 找到它。** Git Agent 调用系统中的 Git，安装包不包含 Git。可运行 `git --version` 检查。
- 需要身份验证的远程操作依赖 SSH 或 HTTPS 凭据助手。在浏览器中登录 GitHub 并不会自动配置 Git 身份验证。
- 使用 LFS 功能需要单独安装 Git LFS。使用子树功能需要你的 Git 安装中包含 `git subtree`。
- Linux 下需要在图形桌面会话中运行。保存 AI 凭据还需要可用且已解锁、兼容 Secret Service 的密钥环。

当前 macOS 打包脚本使用临时签名（ad-hoc signing），尚未经过 Developer ID 公证。
如果 macOS 阻止运行下载的应用，请先核实来源，再参考 [Apple 关于打开已下载应用的说明](https://support.apple.com/en-us/102445)。
不要为了运行应用而关闭系统级安全检查。

## 快速开始

1. 启动 Git Agent，打开已有仓库、克隆远程仓库，或初始化新仓库。
2. 首次提交或推送前，检查仓库的提交身份和远程配置。远程身份验证使用你的 Git/SSH 配置。
3. 在**工作区**中选择改动文件查看差异，暂存需要提交的改动，填写提交说明并提交。
4. 在**历史**中浏览提交图和过往改动，或通过**搜索**查找提交。
5. 如果合并或变基因冲突暂停，打开冲突文件的合并编辑器，检查结果并保存。解决其余文件的冲突后，再完成合并或变基。

日常 Git 操作不需要配置 AI 模型。对于不熟悉或可能破坏数据的操作，请先在可丢弃的测试仓库中尝试。

## AI 辅助合并

AI 是辅助审查冲突的可选工具，不是无人值守的自动合并服务。

1. 在主程序中打开**设置 → AI**，添加模型配置。
2. 选择 API 格式，填写服务商的基础 URL、API 密钥和模型 ID，并测试连接。
   支持兼容 OpenAI 的 Chat Completions 和兼容 Claude 的 Messages 格式；服务商和模型必须支持合并助手使用的结构化工具调用。
3. 在合并编辑器中打开冲突文件，选择已配置的模型并请求分析。
4. 审查解释和建议改动，应用你认可的建议，并在保存前检查生成的代码。完成合并前，请运行项目测试。

**v1.4.0 新增：** 点击 AI 分析按钮旁的**应用所有建议**，可一次应用全部可执行建议，并通过一次撤销恢复。
仅提供人工处理意见的建议会保留供你审查；如果建议已失效或改动相互冲突，整批都不会应用。
应用建议不会自动保存文件，请检查结果后再保存。

### 隐私与凭据

分析可能将文件内容、冲突上下文、相关源码和 Git 历史发送给配置的模型服务商。
分析私有代码前，请确认服务商的数据政策和仓库使用规范。模型服务商可能收取调用费用。

API 密钥在本地配置中加密保存，所用的加密密钥存储在操作系统的凭据存储中。
这并不代表发送给模型分析的源码已被匿名化。分享配置文件或诊断日志前，请检查并移除敏感信息。

## 从源码构建

请先安装 Git、当前稳定版 Rust 工具链，以及对应平台的原生依赖。
macOS、Linux 和 Windows 的详细步骤见[贡献者环境配置指南（英文）](CONTRIBUTING.md#development-prerequisites)。

macOS/Linux 终端和 Windows PowerShell 中的基础开发流程相同：

```sh
git clone https://github.com/adoin/git-Agent.git
cd git-Agent
cargo build --locked --bins
cargo run --locked --bin git-agent
```

请先构建所有可执行文件：主程序会启动同目录中的差异查看器和合并工具。
在刚克隆的仓库中，仅运行 `cargo run --bin git-agent` 不会构建这两个工具。

构建优化版本：

```sh
cargo build --release --locked --bins
```

输出目录为 `target/release/`，开发构建则为 `target/debug/`：

| 可执行文件 | 用途 |
| --- | --- |
| `git-agent` | 主桌面应用 |
| `git-agent-diff` | 独立双栏差异查看器 |
| `git-agent-merge` | 独立三方合并编辑器 |

Windows 下的可执行文件带有 `.exe` 后缀。脱离 Cargo 直接运行时，请将这三个可执行文件放在同一目录。
测试方法、可选的 Windows 开发监听器、独立工具使用示例和打包说明，见 [CONTRIBUTING.md（英文）](CONTRIBUTING.md)。

## 配置与主题

应用设置和仓库标签页状态保存在本地：

| 平台 | 设置目录 |
| --- | --- |
| Windows | 可执行文件旁的 `data/`；安装版为 `<安装目录>/data/` |
| macOS | `~/Library/Application Support/Git Agent/` |
| Linux | `$XDG_DATA_HOME/git-agent/`；未设置时为 `~/.local/share/git-agent/` |

以上路径用于应用设置。诊断日志和语法插件目前使用可执行文件旁的独立 `data/` 目录，macOS 和 Linux 也是如此。
这对安装版的影响见[常见问题](#常见问题)。

可以在设置中调整外观。自定义配色以应用内置的 [theme.json](theme.json) 为基础，
再按以下顺序查找并叠加第一个有效的外部主题文件：

1. `GIT_AGENT_THEME` 环境变量指定的文件。
2. 可执行文件旁的主题文件。
3. 当前工作目录中的主题文件。

在外部主题文件旁放置 `theme.local.json`，即可只覆盖选定的主题配置项。
仓库根目录中的 `theme.local.json` 已被 Git 忽略。修改主题文件后请重启应用。
macOS 下建议使用外部主题文件，不要修改已安装的应用包。

## 常见问题

- **Git 操作或身份验证失败：** 先检查 `git --version`，再在同一仓库的终端中尝试执行失败的 Git 操作。检查 Git 凭据助手或 SSH 配置。
- **差异或合并窗口打不开：** 确认三个可执行文件都在同一目录。使用源码构建时，重新运行 `cargo build --locked --bins`。
- **AI 设置无法保存或解密：** 检查操作系统的凭据存储是否可用且已解锁。将 `config.json` 复制到另一台机器并不会同时复制其加密密钥。
- **macOS/Linux 下找不到日志：** 诊断日志目前尝试写入可执行文件旁的 `data/`，安装后的应用可能没有该目录的写入权限。
  为便于复现和反馈，可在有写入权限的源码目录中构建运行，日志位于 `target/debug/data/`。
- **Windows 下因可执行文件被占用而构建失败：** 构建前关闭主程序、差异查看器和合并编辑器。可选的 Windows 开发监听器会处理重启。

[反馈问题](https://github.com/adoin/git-Agent/issues)时，请提供操作系统与架构、应用版本、Git 版本、复现步骤，以及预期和实际行为。
尽量使用小型测试仓库复现。发布截图和日志前，请移除凭据、私有 URL 和专有代码。

## 参与贡献

欢迎提交问题报告、完善文档、参与跨平台测试，以及提交目标明确的拉取请求（PR）。
请先阅读 [CONTRIBUTING.md（英文）](CONTRIBUTING.md)，了解开发环境、测试、代码结构和项目约定。
较大的改动请先通过 issue 讨论。

## 支持项目

如果 Git Agent 对你的日常工作有帮助，欢迎通过 Ko-fi 自愿赞助，支持项目持续开发。

[在 Ko-fi 上支持 Git Agent](https://ko-fi.com/adoin)

赞助完全自愿，不会解锁付费功能，也不承诺实现特定需求或提供优先支持。
反馈问题、完善文档和贡献代码同样欢迎。

## 许可证

Git Agent 是**源码可见（source-available）**项目，采用 **Apache License 2.0，
并受 Commons Clause License Condition v1.0 限制**。两部分共同生效，不是任选其一，
也不是未附加限制的 Apache 2.0 或 OSI 批准的开源许可证。
完整条款见 [LICENSE](LICENSE)，原作者及项目来源见 [NOTICE](NOTICE)。

- 在遵守许可条件的前提下，允许个人使用、公司内部使用，包括用 Git Agent 开发商业项目。
- 许可不授予 Commons Clause 定义的“Sell”权利：向第三方收取费用或其他对价，
  提供价值全部或主要来自 Git Agent 功能的产品或服务。这并不等于禁止一切商业活动，
  也不是一概禁止销售真正独立的产品。
- 再分发须遵守许可证和声明保留要求；修改过的文件须显著标注修改，保留适用的原版权及署名声明。
  [NOTICE](NOTICE) 中列明了原作者 adoin 和原仓库地址。
- 不额外添加“禁止自愿赞助或赞助链接”的项目专属条款；某笔款项是否构成“Sell”，须看实际安排。
- 第三方组件仍适用各自的许可证。

以上仅为说明，不替代或增加 [LICENSE](LICENSE) 的条款。
可参阅 [Commons Clause 官方说明](https://commonsclause.com/)。
