-- Trainer: a Story mode mod menu. Open it with F7 (fn + F7 on a MacBook).
--
-- Switches (god mode, no police, infinite ammo...) are applied again every
-- half second, since dying, switching character or getting in a car can
-- undo them.

local menu = ui.menu("Trainer")

-- The switches, read by the loop at the bottom.
local god, noPolice, ragdoll, superJump, fastRun = false, false, false, false, false
local infiniteAmmo, carGod = false, false

local function me()
	return player.playerPedId()
end

local function myCar()
	return ped.getVehiclePedIsIn(me(), false)
end

-- Player ---------------------------------------------------------------------

menu:label("PLAYER")

menu:toggle("God mode", false, "Nothing hurts you", function(on)
	god = on
	if not on then
		player.setPlayerInvincible(player.playerId(), false)
	end
end)

menu:toggle("Never wanted", false, "The police leave you alone", function(on)
	noPolice = on
	if on then
		player.clearPlayerWantedLevel(player.playerId())
		player.setMaxWantedLevel(0)
	else
		player.setMaxWantedLevel(5)
	end
	player.setPoliceIgnorePlayer(player.playerId(), on)
end)

menu:button("Heal and full armour", function()
	local p = me()
	entity.setEntityHealth(p, entity.getEntityMaxHealth(p), p)
	ped.setPedArmour(p, 100)
	player.restorePlayerStamina(player.playerId(), 1.0)
	ui.notify("Healed")
end)

menu:choice("Wanted level", { "0", "1", "2", "3", "4", "5" }, 1, function(index)
	local id = player.playerId()
	if index == 1 then
		player.clearPlayerWantedLevel(id)
	else
		player.setMaxWantedLevel(5)
		player.setPlayerWantedLevel(id, index - 1, false)
		player.setPlayerWantedLevelNow(id, false)
	end
end)

menu:toggle("No ragdoll", false, "You stay on your feet", function(on)
	ragdoll = on
	if not on then
		ped.setPedCanRagdoll(me(), true)
	end
end)

menu:toggle("Super jump", false, function(on)
	superJump = on
end)

menu:toggle("Fast run", false, function(on)
	fastRun = on
	player.setRunSprintMultiplierForPlayer(player.playerId(), if on then 1.49 else 1.0)
end)

-- Weapons --------------------------------------------------------------------

menu:label("WEAPONS")

menu:button("Give all weapons", function()
	local p = me()
	local given = 0
	for _, name in world.weapons() do
		weapon.giveWeaponToPed(p, name, 9999, false, false)
		given += 1
	end
	ui.notify(`{given} weapons`)
end)

menu:toggle("Infinite ammo", false, "No reloading either", function(on)
	infiniteAmmo = on
	if not on then
		weapon.setPedInfiniteAmmoClip(me(), false)
	end
end)

-- Vehicles -------------------------------------------------------------------

menu:label("VEHICLES")

local cars = {
	"adder", "zentorno", "t20", "turismor", "entityxf", "infernus", "comet2", "banshee",
	"kuruma", "insurgent", "sanchez", "bati", "akuma", "buzzard", "frogger", "luxor",
	"lazer", "dinghy", "jetmax", "rhino",
}
local carChoice = menu:choice("Vehicle", cars, 1)

menu:button("Spawn it", "You get in, the old one stays behind", function()
	local model = cars[carChoice.value]
	if not streaming.isModelInCdimage(model) or not streaming.isModelAVehicle(model) then
		ui.notify(`{model} isn't in this game`)
		return
	end
	streaming.requestModel(model)
	local started = os.clock()
	while not streaming.hasModelLoaded(model) do
		if os.clock() - started > 5 then
			ui.notify(`{model} didn't load`)
			return
		end
		wait()
	end
	local p = me()
	local spot = entity.getEntityCoords(p, true) + entity.getEntityForwardVector(p) * 5
	local car = vehicle.createVehicle(model, spot, entity.getEntityHeading(p), false, false, false)
	streaming.setModelAsNoLongerNeeded(model)
	if not car then
		ui.notify("No room for another vehicle")
		return
	end
	vehicle.setVehicleOnGroundProperly(car, 5.0)
	ped.setPedIntoVehicle(p, car, -1)
end)

menu:button("Fix this vehicle", function()
	local car = myCar()
	if car then
		vehicle.setVehicleFixed(car)
		vehicle.setVehicleDirtLevel(car, 0)
		ui.notify("Fixed")
	else
		ui.notify("Get in a vehicle first")
	end
end)

menu:toggle("Unbreakable vehicle", false, "The one you are in", function(on)
	carGod = on
	local car = myCar()
	if car and not on then
		entity.setEntityInvincible(car, false)
		vehicle.setVehicleCanBreak(car, true)
	end
end)

-- World ----------------------------------------------------------------------

menu:label("WORLD")

menu:slider("Time of day", 12, 0, 23, 1, function(hour)
	clock.setClockTime(hour, 0, 0)
end)

menu:toggle("Freeze time", false, function(on)
	clock.pauseClock(on)
end)

local weathers = { "EXTRASUNNY", "CLEAR", "CLOUDS", "OVERCAST", "SMOG", "FOGGY", "RAIN", "THUNDER", "CLEARING", "SNOW" }
local weatherNames = { "Extra sunny", "Clear", "Clouds", "Overcast", "Smog", "Foggy", "Rain", "Thunder", "Clearing", "Snow" }
menu:choice("Weather", weatherNames, 1, function(index)
	misc.clearOverrideWeather()
	misc.setWeatherTypeNowPersist(weathers[index])
end)

menu:button("Teleport to map marker", "Set one on the pause map first", function()
	local marker = hud.getFirstBlipInfoId(8) -- 8: the waypoint
	if not marker or not hud.doesBlipExist(marker) then
		ui.notify("No marker on the map")
		return
	end
	local at = hud.getBlipInfoIdCoord(marker)
	local car = myCar()
	local mover: Entity = if car then car else me()
	-- The ground there is not loaded yet: try heights from high to low
	-- until the game knows where it is.
	for height = 1000, 0, -50 do
		local probe = vector.create(at.x, at.y, height)
		entity.setEntityCoordsNoOffset(mover, probe, false, false, false)
		streaming.requestCollisionAtCoord(probe)
		wait(50)
		local found, z = misc.getGroundZFor3dCoord(probe, false, false)
		if found then
			entity.setEntityCoordsNoOffset(mover, vector.create(at.x, at.y, z + 1), false, false, false)
			ui.notify("Teleported")
			return
		end
	end
	entity.setEntityCoordsNoOffset(mover, vector.create(at.x, at.y, 100), false, false, false)
	ui.notify("Teleported (no ground found)")
end)

-- Keep the switches on -------------------------------------------------------

local function keepOn()
	local id, p = player.playerId(), me()
	if god then
		player.setPlayerInvincible(id, true)
	end
	if noPolice then
		player.clearPlayerWantedLevel(id)
	end
	if ragdoll then
		ped.setPedCanRagdoll(p, false)
		ped.setPedCanBeKnockedOffVehicle(p, 1)
	end
	if infiniteAmmo then
		weapon.setPedInfiniteAmmoClip(p, true)
	end
	if carGod then
		local car = myCar()
		if car then
			entity.setEntityInvincible(car, true)
			vehicle.setVehicleCanBreak(car, false)
			vehicle.setVehicleFixed(car)
		end
	end
end

local lastKeep = 0
while true do
	-- Super jump lasts one frame; the rest every half second.
	if superJump then
		misc.setSuperJumpThisFrame(player.playerId())
	end
	if os.clock() - lastKeep > 0.5 then
		lastKeep = os.clock()
		keepOn()
	end
	wait()
end
