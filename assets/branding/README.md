# Synapse 品牌图标

## 设计概念

- 单一连续折带同时表达笔记页面与思维连接，避免多个装饰元素分散注意力。
- 折带形成克制的抽象 `S` 轮廓，对应 Synapse 名称，但不依赖文字识别。
- 暖白色内嵌折角代表 Markdown 文档与写作。
- 黑曜石与深石墨材质延续应用的深色界面，轻微冷蓝边缘反光在不使用高饱和渐变的前提下保持层次。

## 交付文件

- `synapse-app-icon.png`：1024 × 1024 RGB、全画布不透明的未遮罩正方形主图标。macOS 由系统统一应用圆角遮罩，不应在源图中预制透明圆角。
- `synapse-app-icon.icns`：macOS App Bundle 图标，包含 16、32、128、256、512 和 1024 像素 Retina 尺寸。
- `synapse-app-icon.ico`：Windows 可执行文件与安装包图标，包含 16、32、48 和 256 像素 PNG 帧。由 `scripts/generate-windows-icon.py` 从 PNG 主资源生成。

图标不包含文字或第三方商标，为 Synapse 项目生成的原创资产。

## macOS 接入

- `cargo run -p synapse`：PNG 会编译进可执行文件，并通过 AppKit 设置 Dock 和应用切换器图标，方便开发时识别未打包的二进制。
- 从 `.app` / DMG 启动时不覆盖图标，改用 bundle 内的 `.icns`，由 macOS 统一加上圆角遮罩（Dock、应用程序列表、`Command+Tab`）。
- `./scripts/package-macos.sh`：生成 `target/release/bundle/osx/Synapse.app`。
- `./scripts/package-macos.sh --dmg --universal`：生成通用架构 `.app` 和可拖到「应用程序」的 DMG。
- `./scripts/package-macos.sh --install`：生成应用包并安装到 `/Applications/Synapse.app`，供 Finder、Launchpad 和固定 Dock 快捷入口使用。
- `./scripts/package-windows.ps1`：在 Windows 上生成 `Synapse-<version>-windows-x64.exe`。
