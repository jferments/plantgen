# Fonts

PlantLab's type, as WorldLab's redesigned interface draws it (Joshi,
2026-10-11). `src/ui/theme.rs` builds these files into `plantlab` with
`include_bytes!` and `theme::fonts` installs them. They are the upstream
release files and licence texts, unmodified.

| File | Font | Version | Licence | Source |
| --- | --- | --- | --- | --- |
| `Inter-Regular.ttf` | Inter, regular | 4.1 | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) | `extras/ttf/` of [`Inter-4.1.zip`](https://github.com/rsms/inter/releases/tag/v4.1), The Inter Project Authors |
| `Inter-SemiBold.ttf` | Inter, semibold | 4.1 | SIL Open Font License 1.1 (`Inter-LICENSE.txt`) | the same archive |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono, regular | 2.304 | SIL Open Font License 1.1 (`JetBrainsMono-OFL.txt`) | `fonts/ttf/` of [`JetBrainsMono-2.304.zip`](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304), The JetBrains Mono Project Authors |

SHA-256 of each file:

```text
40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82  Inter-Regular.ttf
78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3  Inter-SemiBold.ttf
a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f  JetBrainsMono-Regular.ttf
```

The archives' own SHA-256: `Inter-4.1.zip`
`9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`,
`JetBrainsMono-2.304.zip`
`6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`.

The icons are not here: they are Phosphor's regular weight (MIT), which
the `egui-phosphor` 0.14.0 crate (MIT or Apache 2.0) carries and
`theme::fonts` adds.
