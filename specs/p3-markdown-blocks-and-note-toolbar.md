# P3 — Markdown 块级呈现与笔记顶部操作

Reference implementation:

- <https://github.com/starc007/markd/blob/main/src/styles.css#L273-L397>
- <https://github.com/starc007/markd/blob/main/src/components/layout/AppShell.tsx>
- <https://github.com/starc007/markd/blob/main/src/components/editor/NoteBreadcrumb.tsx>

## Functional requirements

- FR-1：连续引用行渲染为一个视觉连续的引用块。左侧使用无圆角 2px `ink` 竖线，正文使用 muted 色；块外上下间距为 0.8em，内容相对竖线缩进 1em。
- FR-2：H1-H4 分别使用 1.6/1.3/1.1/1.0em 字号和 620/580/560/550 字重；H5/H6 使用 1em 正文字号与普通字重。所有标题均使用主题 `ink`，禁止 writ 的 heading bold run 再次叠加为 700。
- FR-3：围栏代码块在阅读状态隐藏开闭围栏，内容使用连续 panel 表面、1px line-soft 边框、8px 圆角、14×16px 内边距和 0.86em 等宽字体。
- FR-4：writ/tree-sitter 产生的代码高亮颜色必须映射到 GPUI `TextRun`，同时保留源码 Unicode 索引、选区、输入法与保存内容。
- FR-5：页签下方显示 40px 笔记工具行。左侧面包屑由活动笔记相对路径生成，文件夹使用 muted 色，当前笔记去除 `.md` 后缀并使用 semibold `ink`；路径过长时单段截断。
- FR-6：工具行右侧提供至少 40×40px 命中区的源码模式按钮；源码模式按原始 Markdown、等宽 13px 字体和恒等源码/显示索引渲染，再次点击恢复实时呈现模式。
- FR-7：更多菜单提供 Export as Markdown、Copy Markdown、Delete Note。导出使用系统保存面板，复制使用系统剪贴板，删除复用未保存保护与系统废纸篓实现。
- FR-8：页签、文件夹、笔记及笔记操作菜单项必须显示语义匹配的 Lucide 图标；图标必须位于菜单名称左侧并共享固定 18px 图标列，SVG 使用 15px 尺寸、禁止 flex 收缩并显式设置普通/危险主题色。
- FR-9：编辑器支持平台标准格式快捷键：`Mod+B` 粗体、`Mod+I` 斜体、`Mod+U` 下划线、`Mod+Shift+S` 删除线、`Mod+E` 行内代码、`Mod+Alt+C` 围栏代码块。非空选区被标记包裹，再次执行可移除已有标记；空光标插入成对标记并停在中间。
- FR-10：行首输入至少三个反引号或波浪线及可选语言标识后按 Enter，自动生成内容行和同长度闭合围栏，光标停在内容行；若下方已有匹配闭合围栏则只插入内容行，不得重复闭合。普通段落中的内联围栏文本不得触发转换。
- FR-11：macOS 英文输入状态下，编辑器必须同时接受直接反引号和 `Option + 数字 1 左侧的波浪号键` 产生的反引号；后者必须按字面插入，不得被 GPUI 的修饰键匹配吞掉。
- FR-12：光标位于围栏代码块末尾时，连续三次回车必须删除代码块末尾的两个空行并把光标移动到闭合围栏后的普通段落；文档以代码块结尾时，点击代码块下方空白区域必须创建可编辑的块后段落，不能继续把光标映射回代码内容。

## Acceptance criteria

- AC-1：浅色和深色主题下引用竖线分别跟随深色/亮色 `ink`，无圆角、无断裂。
- AC-2：H1-H6 的颜色保持一致，字号和字重逐级收敛；H3-H6 不再全部显示为 17.6px semibold。
- AC-3：输入带语言名称的三反引号代码块后，非活动围栏隐藏，代码显示在完整容器内，并使用 writ 支持的语法高亮。
- AC-4：源码模式显示原始标题、引用、围栏和表格符号，光标与 Unicode 文本索引保持一致；切回后磁盘 Markdown 不变。
- AC-5：面包屑正确显示多层中文路径和无后缀笔记标题；两个操作按钮保持 40px 命中区。
- AC-6：导出文件内容、复制内容与活动文档源码完全一致；脏文档删除仍被会话安全规则拒绝。
- AC-7：所有右键菜单和笔记更多菜单在浅色/深色主题中同时显示图标与文本；每一行都按“左侧图标、右侧名称”排列，图标列和名称起始位置纵向对齐。
- AC-8：格式化、workspace/all-targets 严格 Clippy、全量测试、开发构建和 `git diff --check` 通过；不启动应用、不运行截图测试。
- AC-9：中英文选区和空光标都能通过格式快捷键生成正确 Markdown 与光标位置；`````rust`` 和 `~~~typescript` 回车后立即形成可输入且最终保存为标准 Markdown 的完整代码块。
- AC-10：代码块末尾前两次回车保留在代码块内，第三次回车跳出且不留下多余空代码行；键盘退出和鼠标点击块后空白均可立即输入普通 Markdown 正文。
