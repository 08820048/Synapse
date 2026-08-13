# P4 — Mermaid 原生渲染与文件树右键交互

参考实现与技术来源：

- Markd 文件树右键交互：<https://github.com/starc007/markd/blob/main/src/components/tree/FileTree.tsx>
- Markd 菜单定位：<https://github.com/starc007/markd/blob/main/src/components/ui/ContextMenu.tsx>
- Rusty Mermaid：<https://github.com/base58ed/rusty-mermaid>

Markd 当前源码没有 Mermaid 渲染实现，因此本阶段只参考其文件树右键交互；Mermaid 使用 Rusty Mermaid 解析并由 GPUI 原生显示 SVG，不引入 WebView、浏览器或 JavaScript 运行时。

## Functional requirements

- FR-1：非源码模式识别语言名为 `mermaid` 的反引号或波浪线围栏代码块并持续显示图表；光标进入块内或点击图表均不得自动恢复围栏源码，完整 Mermaid 源码通过右上角 Markdown 源码模式统一查看和编辑。
- FR-2：Mermaid 源码、Unicode 字符范围和保存内容保持不变；图表渲染不得改写 Markdown 文本。
- FR-3：图表使用与 Synapse 浅色/深色主题一致的背景、节点、线条和文字颜色，并在正文实际可用宽度内等比缩放。
- FR-4：同一笔记 revision 和主题下复用已经生成的 SVG；普通光标移动不得重新解析 Mermaid。内容、主题或源码模式改变时才失效。
- FR-5：非法 Mermaid 不得导致编辑器崩溃；对应块显示可辨识的错误卡片，用户仍可点击进入源码修正。
- FR-6：文件夹与笔记行使用普通 pointer 光标；拖拽手势开始前不得显示抓手光标。
- FR-7：右键菜单以原生右键按下的窗口坐标为锚点，通过 GPUI `deferred(anchored())` 根据面板实测尺寸限制在窗口 8px 安全边距内；禁止把窗口坐标直接交给普通 flex 父节点下的绝对定位。
- FR-8：打开一个文件树菜单后，直接右键另一行必须能在同一手势中关闭旧菜单并打开新菜单；不得使用覆盖整窗、阻断下一次目标命中的透明 backdrop。
- FR-9：页签、文件树和笔记操作右键菜单内部不显示菜单项分割线，所有菜单项保持连续的一体化面板。

## Acceptance criteria

- AC-1：测试文档中的 flowchart 与 sequenceDiagram 围栏各生成一个 SVG 预览锚点；光标位于块外时不重复显示围栏和内容行。
- AC-1.1：光标进入 Mermaid 块任意源码范围时仍只显示单一图表预览锚点，源码/显示索引保持一致；切换右上角源码模式后开围栏、全部图表源码和闭围栏逐字可见。
- AC-2：浅色和深色 Mermaid 主题分别使用编辑器背景 `#fbfbfa` 与 `#1a1a1a`，SVG 尺寸始终为正数并受正文宽度约束。
- AC-3：移动光标但不修改内容时复用同一份 Mermaid 预览缓存；编辑内容或切换主题后重新生成。
- AC-4：文件树和页签菜单均使用窗口坐标模式的 GPUI `Anchored`，菜单位置不受侧栏滚动、flex 静态位置或父级布局原点影响，并根据实际测量尺寸贴合窗口边缘。
- AC-5：文件树行无 `cursor_move`；菜单没有全屏 context backdrop，也没有菜单项分割线。
- AC-6：格式化、workspace/all-targets 严格 Clippy、全量测试、开发构建和 `git diff --check` 通过；不启动应用、不运行截图测试。
