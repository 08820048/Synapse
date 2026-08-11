# Bundled fonts

Synapse vendors the normal Inter variable font used by the Markd reference UI.

- File: `Inter-Variable.ttf`
- Family exposed to GPUI: `Inter`
- Axes: optical size (`opsz`) 14–32 and weight (`wght`) 100–900
- Upstream: Google Fonts `ofl/inter/Inter[opsz,wght].ttf`
- Upstream Inter revision recorded by Google Fonts: `66647c0bbbe41a850d79d9c76fb13add3378940f`
- Fontsource package used by Markd: `@fontsource-variable/inter 5.2.8`
- SHA-256: `29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031`
- License: SIL Open Font License 1.1; see `OFL-Inter.txt`

The TTF is registered directly with GPUI at startup. It is not downloaded at runtime and does not add a WebView, CSS font loader, or JavaScript dependency.
