# Bundled fonts

Synapse vendors the normal and italic Inter variable fonts used by the Markd reference UI, plus concrete italic, bold and bold-italic faces for GPUI's static font-face matcher.

- Runtime files: `Inter-Variable.ttf`, `Inter-Italic.ttf`, `Inter-Bold.ttf`, `Inter-BoldItalic.ttf`
- Source variable face retained for reproducibility: `Inter-Italic-Variable.ttf`
- Family exposed to GPUI: `Inter`
- Axes: optical size (`opsz`) 14–32 and weight (`wght`) 100–900
- Upstream: Google Fonts `ofl/inter/Inter[opsz,wght].ttf`
- Upstream Inter revision recorded by Google Fonts: `66647c0bbbe41a850d79d9c76fb13add3378940f`
- Fontsource package used by Markd: `@fontsource-variable/inter 5.2.8`
- SHA-256: `29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031`
- Bold SHA-256: `9f4a9caffbd1033874adbe0c6ad1792aa0cfaf3cc939076d3d11ba60cf522932`
- Italic SHA-256: `acd98e64795781b2058f07b18475e0ecee2a0fe2b42a49e2f9e37d0d6bf66ce6`
- Concrete Italic SHA-256: `08bf85179da021ea50864d285ae9f24c4c605b581110e2baa47e3fed32adad09`
- Bold Italic SHA-256: `0773663dcfe384e5d01e08851a63da5141a6889030fff2f3f8dc260956eca68f`
- License: SIL Open Font License 1.1; see `OFL-Inter.txt`

`Inter-Italic.ttf`, `Inter-Bold.ttf` and `Inter-BoldItalic.ttf` are weight-400/700 instances generated from the matching official variable TTFs with FontTools. Their OS/2 tables explicitly advertise their weight and corresponding bold/italic selection bits. GPUI/macOS matches registered faces by these static font properties, so the concrete faces prevent emphasis runs from resolving back to the regular variable face, including combined strong-emphasis syntax. All runtime TTFs are registered directly with GPUI at startup; none are downloaded at runtime, and they add no WebView, CSS font loader, or JavaScript dependency.

Inter does not contain CJK glyphs, and macOS normally falls an italic Inter run back to upright PingFang. Synapse therefore supplies native serif/calligraphic CJK fallback families for Markdown emphasis runs (`Kaiti SC` first on macOS, with platform equivalents elsewhere). This keeps Latin text on the real Inter Italic face while giving Chinese emphasis a visibly distinct native glyph form instead of leaving it identical to upright body text.
