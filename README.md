# Synapse

Synapse 是一款使用 Rust 与 GPUI 构建的高性能、本地优先 Markdown 编辑器。

产品重点是快速启动、低内存、可靠的本地文件编辑，以及接近 Zed 的原生桌面体验。Markdown 文件和本地文件夹是唯一数据源，不依赖 Web 技术、数据库或强制网络服务。

## 当前能力

- 在应用内通过系统文件夹选择器打开本地 Markdown 目录。
- 递归发现 `.md` / `.MD` 文件和空文件夹，并显示可展开/收起的分层文件树；展开目录使用 `folder-open`，空目录显示“空文件夹”。
- 在 Vault 根目录或任意子目录直接创建 `未命名N` 文件夹或 Markdown 笔记，不需要先填写命名弹层。
- 新建笔记会立即打开并写入 `# 未命名N`；编辑该一级标题会同步更新对应文件名。
- 文件夹右键支持新建子文件夹、新建笔记、重命名、Finder 定位和移到废纸篓。
- 笔记右键支持重命名、Finder 定位和移到废纸篓；危险操作使用红色提示。
- 文件夹和笔记重命名在文件树原位置进行，支持中文 IME；Enter 提交，Escape 取消。
- 文件夹和笔记支持拖到目标目录、同级笔记所在目录或 Vault 根目录。
- 多文档页签、独立 Rope 缓冲区、Unicode 光标和脏状态。
- 页签切换、关闭，以及 Close / Close Left / Close Right / Close All 右键菜单。
- GPUI 原生文本输入，支持中文 IME、鼠标点击定位、换行、删除和上下左右/Home/End 光标导航。
- 普通 Enter 使用 `writ 0.18.1` 的 tree-sitter Markdown 列表上下文：自动续写无序列表、有序列表、任务列表和引用；空列表项回车退出列表，Shift+Enter 插入原始换行。
- 编辑光标按约 530ms 周期闪烁；输入、鼠标定位或方向移动后立即恢复显示并重置闪烁周期。
- Markdown 源码在内存和磁盘中保持原样，编辑区实时呈现标题、列表、引用、代码及常用行内标记。
- 光标所在行会恢复显示原始 Markdown 标记，离开该行后重新显示阅读样式，避免隐藏语法时无法准确编辑结构。
- 编辑器使用 `w-full + 1120px max-width` 的响应式居中写作页，按实际编辑区宽度应用 16/24/32px 水平边距；正文为 16px 系统字体与 1.65 行高，不显示行号，活动行 Markdown 标记使用弱化灰色。
- 文档页签直接位于 44px 自定义标题栏；长标题在固定页签宽度内以省略号收尾。侧栏收起会立即释放全部宽度，页签和编辑区同步左移扩展。
- `Cmd/Ctrl + S` 原子保存，保存失败不会丢失内存缓冲区。
- 可收起的左侧导航和一体化编辑区域。
- Lucide `panel-left` / `panel-right` 侧栏开关位于首个页签左侧，保持 40px 命中区；编辑器不再显示底部工具栏。
- 左侧 Lucide 搜索入口使用 `Search any...` 文案和右侧 `⌘K` 徽标，可打开中央命令面板。
- 界面已接入 `gpui-component 0.5.1`：统一使用 Root、Theme、Button、Input 与 Kbd，`Cmd/Ctrl+K` 可在任意区域打开并聚焦命令搜索。
- Settings 的 Appearance 面板支持 System、Light、Dark 三态并持久化；浅色和深色模式均使用不同的侧栏与写作画布表面色。
- Todo、Bookmarks 和 Settings 入口 UI；侧栏图标显式使用主题弱化前景色，浅色/深色模式均可见。
- 无序列表不再直接显示系统字体中的 `•` 字形：GPUI 在 writ 保留的标记槽位中独立绘制 5px 圆形 marker，并使用与 Markd `--faint` 一致的主题层级；`-`、`*`、`+` 均统一呈现。序号同样使用 faint 色。
- 导航、菜单和页签操作统一使用编译进应用的 Lucide 官方 SVG 图标。

搜索、待办、书签和其余设置业务逻辑将在后续版本实现；主题设置已经可用。V3 文件操作已经接入真实文件系统，并对路径穿越、符号链接、覆盖冲突、递归移动和未保存页签提供保护。

## 技术约束

- 语言：Rust 最新稳定版。
- UI：GPUI，禁止引入 HTML/CSS/JavaScript、Electron、egui 或 iced。
- 组件体系：最新正式版 `gpui-component 0.5.1` 与 `gpui-component-assets 0.5.1`；标准按钮、输入、快捷键标签和浮层宿主优先使用组件库实现。
- 文本缓冲区：ropey。
- 删除：使用 `trash 5.2.6` 移入操作系统废纸篓，不执行永久删除。
- 图标：固定使用 Lucide `1.27.0`，项目只内置实际使用的 SVG，并随附上游许可证与来源记录。
- 动画：固定使用 `gpui-animation 0.2.63`，交互时长和缓动遵循 `docs/过渡和交互动画规约.md`。
- Markdown 编辑内核：固定接入 `writ 0.18.1` 并关闭默认 app feature；当前先复用其无头 `EditorState` 和 tree-sitter-md 列表上下文，GPUI 继续承担应用壳、输入法和绘制。
- Markdown 呈现：当前仍是可编辑的轻量块级实时呈现；后续分阶段把 writ 的解析、位置映射和更多语法节点接入 GPUI 渲染层。
- 文件监听：后续使用 notify。
- 文件系统始终是 Markdown 文档的真实来源。

架构保持清晰分层：

- `synapse-core`：Vault、Markdown 文档、路径安全和持久化。
- `synapse-ui`：GPUI 窗口、页签、导航、命令面板和编辑会话状态。
- 后续独立模块：专业编辑输入、Markdown 渲染、搜索、配置。

## 运行

```bash
cargo run -p synapse
```

也可以直接传入初始目录：

```bash
cargo run -p synapse -- /path/to/markdown-folder
```

## 编辑快捷键

| 操作 | 快捷键 |
|---|---|
| 保存 | macOS `Cmd+S`，Windows/Linux `Ctrl+S` |
| 换行 | `Enter` |
| 原始换行（不续写 Markdown 容器） | `Shift+Enter` |
| 删除 | `Backspace` / `Delete` |
| 移动光标 | `Up` / `Down` / `Left` / `Right` / `Home` / `End`，或鼠标点击文本位置 |

## 验证

```bash
cargo fmt --package synapse-core -- --check
cargo fmt --package synapse -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

当前开发进度见 [PROGRESS.md](PROGRESS.md)，版本规划见 [docs/Task.md](docs/Task.md)。
