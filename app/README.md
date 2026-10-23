# re5OSX: the app

The GTA V web build as a native Mac app: the game's WebAssembly is translated to C (wasm2c) and compiled for Apple
silicon, its Direct3D calls run on Metal through wgpu, and SDL handles the window, input, sound and controllers.

## Building

Needs Xcode 26 or later (for the app icon), [mise](https://mise.jdx.dev), which installs Rust, CMake and the
packager on first use, and a game folder holding the build's original `b/<build>/game.wasm`.

```sh
export RE5OSX_GAME_DIR=/path/to/game

mise run macos:download   # target/download/re5OSX.app and re5OSX.dmg: what players download
mise run macos:build      # target/package/re5OSX.app: the game compiled in, for development
mise run macos:run        # open that one
mise run test             # the tests
```

`mise run setup` (run by the builds) fetches the third-party sources into `third_party/`, applies `patches/` and
builds wasm2c.

**re5OSX.app** (`macos:download`) never contains the game's code. It carries a kit: the whole app as a static library
built without the game (`RE5OSX_GAME_LINK=external`), the game's compiler (`re5osx-gamec`) and
`platform/macos/link-app`. On the player's Mac, Install compiles their own `game.wasm` with Apple's Command Line Tools
and links the two into `~/Library/Application Support/re5OSX/game/re5OSX.app`, which re5OSX.app opens from then on.

The tile pictures and menu sounds are read at run time from `~/Library/Application Support/re5OSX/assets`, which
re5OSX.app's setup makes from the game; without them the tiles are plain and the menus quiet.

## Layout

| Crate | |
|---|---|
| `crates/re5osx-launcher` | re5OSX.app: opens the game, or first sets the game folder up and makes the game app |
| `crates/re5osx-app` | The app as a static library without the game's code (the kit's `libre5osx_app.a`) |
| `crates/re5osx-gamec` | `re5osx-game`'s build script as a program: compiles the player's game on their Mac |
| `crates/re5osx` | The app: window, start page and settings (egui), input, sound |
| `crates/re5osx-game` | The game's code, translated and compiled, and the hooks into it |
| `crates/re5osx-runtime` | The runtime under the game: memory, threads, files, input, online play |
| `crates/re5osx-gpu` | The renderer: the game's Direct3D 11 calls on wgpu |
| `crates/re5osx-qos` | Thread priorities for the engine's threads |
| `crates/re5osx-mods` | Luau mods and the mod studio's server |
| `crates/re5osx-studio` | The mod studio's web page |
| `crates/re5osx-stream` | Streaming the game's files from another Mac |

| Folder | |
|---|---|
| `platform/macos` | Info.plists, and `link-app` |
| `platform/shared` | The app icon (Icon Composer) |
| `patches` | Our changes to wasm2c, wgpu and FSR 2, applied by `mise run setup` |
| `.mise/tasks` | The `mise run` tasks |

How it all works is in [docs/design.md](../docs/design.md); mods are in [docs/mods.md](../docs/mods.md).

## License

GPL-3.0-or-later ([LICENSE](LICENSE)); see [NOTICE](../NOTICE).
