# Mods and re5OSX Studio

Mods are small [Luau](https://luau.org) scripts that run inside the game,
as the game's own scripts do, and call its native functions: spawn a car,
change the weather, add a menu. **re5OSX Studio** is the editor for them,
served by the app to your browser, on the same Mac or on another computer.


## Using mods

1. Turn mods on: the **Mods** page (start page, Settings › Game › Mods, or
   F7 in the game), its **Mods** switch.
2. Put mods in the mods folder, one folder each, or import a `.zip` in the
   Studio. The folder is `mods` in the user folder
   (`~/Library/Application Support/re5OSX`).
3. Start a game. Mods marked to start with the game run once you are in;
   the others start from the Mods page or the Studio.

For an example, copy [`mods/trainer`](../mods/trainer) from this
repository into the mods folder: a Story mode trainer (god mode, no police,
weapons, vehicles, time, weather, teleporting to the map marker) that opens
with F7.

Mods with a menu show it on the Mods page's **MENUS** (F7), drawn by the
app: it works with controllers, keyboard and mouse.

## re5OSX Studio

The Mods page's **Studio** tab starts it and shows its address, its
pairing code and a QR code. Open it in a browser on the same Mac, or
turn on **Other devices** and open the address on another computer.


- **An editor that knows the game.** Every native is typed and documented,
  with completions, hovers and signatures as you type. Model names
  (vehicles, peds, weapons) come from your installed game, so a misspelt
  car is an error before it runs.
- **Live.** Saving restarts the mod in the running game. The console runs
  Luau in the game as you type it; each mod's log and errors come back
  with the line they came from.
- **Menus.** The mods' menus, mirrored from the game: use them from the
  Studio or from the game itself.
- **Mod pages.** Name, description, version, whether it starts with the
  game; export a mod as a `.zip`, import one.
- **Safe to connect.** Only a browser with the pairing code gets in, and
  only from the Studio's own page. A new code (Mods page) closes every
  connection made with the old one.

## Your first mod

A mod is a folder with a `main.luau`, and a `mod.json` that describes it.
This one adds a menu that delivers a car, and sets the weather and the
hour:

```lua
--!strict
-- Garage: any car, any weather, any hour, from the Mods menu (F7).

local cars = { "adder", "zentorno", "t20", "sanchez", "deluxo" }
local weathers = { "EXTRASUNNY", "CLEAR", "CLOUDS", "RAIN", "THUNDER", "FOGGY", "XMAS" }

local function deliver(model: string)
	local hash = joaat(model)
	streaming.requestModel(hash)
	while not streaming.hasModelLoaded(hash) do
		wait()
	end
	local me = player.playerPedId()
	local car = vehicle.createVehicle(hash, entity.getEntityCoords(me, true), entity.getEntityHeading(me), false, false, false)
	if car then
		ped.setPedIntoVehicle(me, car, -1)
		ui.notify(`Your {model} is here`)
	end
end

local garage = ui.menu("Garage")
local car = garage:choice("Car", cars, 2)
garage:button("Deliver", "Right where you stand", function()
	deliver(cars[car.value])
end)
garage:choice("Weather", weathers, 1, function(_, weather)
	misc.setWeatherTypeNowPersist(weather)
end)
garage:slider("Hour", 19, 0, 23, 1, function(hour)
	clock.setClockTime(hour, 0, 0)
end)
```

```json
{
  "name": "Garage",
  "description": "Any car, any weather, any hour: a menu for the open world.",
  "version": "1.0",
  "autostart": true
}
```

## What a mod can use

| | |
|---|---|
| **Natives** | The game's script functions by namespace, named the Luau way: `vehicle.createVehicle(...)`, `player.playerPedId()`. Outputs come back as extra return values; a handle of 0 as `nil`. |
| `wait(ms)` | Back to the game for `ms` milliseconds (none: until the next frame). |
| `ui.menu(title)` | A menu: `toggle`, `slider`, `choice`, `button`, `label`. `ui.notify(text)` shows a notification. |
| `task.spawn`, `task.delay` | Coroutines on the game's clock (they stop while the game is paused). |
| `world` | The vehicles, peds and objects around, and the installed game's model names. |
| `json`, `storage` | JSON text, and values kept between sessions (1 MB a mod, for this player). |

Files and the network are not available to mods.

**A mod cannot take the game down.** An error stops that mod with its
message and traceback; arguments are checked before a native runs; a mod
gets at most 2 ms a frame, and one that runs 2 seconds without waiting is
stopped.

How mods run inside the engine is in the [design notes](design.md#mods).
