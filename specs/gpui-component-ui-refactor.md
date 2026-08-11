# GPUI Component UI 重构规格

## 目标

在不替换 Synapse 文件、会话和 Markdown 编辑逻辑的前提下，以 `gpui-component` 的主题、根容器和标准控件重构应用界面，统一交互状态、焦点、快捷键提示与浮层视觉。

## 版本策略

- GPUI 使用 crates.io 最新正式版 `0.2.2`。
- `gpui-component` 使用最新正式版 `0.5.1`。
- `gpui-component-assets` 使用最新正式版 `0.5.1`，并与 Synapse 自有 Lucide 资源合并。
- `gpui-component 0.5.1` 的清单在 `vendor/gpui-component` 本地供应，仅将 `tree-sitter` 依赖从 `0.25` 对齐到 writ 使用的 `0.26`；组件源码与功能行为保持上游原样。
- 不追踪要求不稳定 Rust API 的 GPUI 主分支，项目继续使用 Rust 最新稳定版构建。

## 功能要求

- FR-1：应用启动时必须先调用 `gpui_component::init`，再应用已保存的 System、Light 或 Dark 主题偏好；首次启动默认跟随系统。
- FR-2：窗口第一层必须使用 `gpui_component::Root`，为 Dialog、Sheet、Popover、Notification 和输入焦点管理提供统一宿主。
- FR-3：应用背景、侧栏、页签、弹层、边框、状态色和文本色必须优先读取组件主题 token，语法高亮专用色除外。
- FR-4：普通操作按钮、图标按钮、危险按钮和菜单操作必须使用 `gpui_component::Button`。
- FR-5：命令面板搜索区域必须使用 `gpui_component::Input`，支持系统输入法、选择、复制粘贴和组件焦点状态。
- FR-6：搜索入口必须使用 `gpui_component::Kbd` 展示当前平台快捷键，并支持全局 `Cmd+K` / `Ctrl+K` 打开与聚焦搜索输入。
- FR-7：组件默认图标资源与项目固定的 Lucide 资源必须通过同一个 GPUI `AssetSource` 可用。
- FR-8：文件树拖拽、Markdown 编辑器、自定义折行绘制和原位重命名等领域专用交互保留现有实现，不因视觉组件迁移丢失功能。

## 验收标准

- AC-1：依赖树中只有一个 GPUI 和一个 tree-sitter 原生链接版本。
- AC-2：空状态、侧栏快捷入口、新建控件、页签关闭、页签右键菜单、文件右键菜单、工具栏、底栏开关和命令面板操作均由标准 Button 构建。
- AC-3：命令面板输入使用标准 Input，打开后获得焦点；Kbd 标签跟随 macOS 与非 macOS 快捷键。
- AC-4：所有 workspace 测试、Clippy 严格检查和开发构建通过。
- AC-5：按产品要求不运行截图测试或自动 UI 操作，最终视觉和输入法候选窗由产品手动验收。
