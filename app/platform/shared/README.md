# re5OSX app icon

`re5OSX.icon` is the app icon's Icon Composer source (open it with the Icon
Composer that comes with Xcode 26): a white V, `Assets/01-v.png` (1024 × 1024,
transparent), on a solid black fill. `icon.json` has no glass, shadow or
specular effects, so the icon stays flat black and white.

`mise run macos:build` and `mise run macos:download` compile it with `actool`
into `Assets.car` and the fallback `re5OSX.icns` for older macOS versions; both
go into each app bundle. Do not round the PNG's corners: macOS applies its own
mask.
