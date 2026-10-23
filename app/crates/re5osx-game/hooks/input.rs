// Hooks for the runtime's `input.rs` (see ../hooks.rs).
&[
    // Once per frame for each of the 4 pads: feeds native gamepads (this build
    // has no XInput and never fills the DirectInput stick table).
    ("pad_update", "rage::ioPad::Update(bool)"),
    // A Scaleform method the scripts call, with its float arguments: the
    // phone's web browser gets mouse clicks it understands.
    ("scaleform_begin_method", "CScaleformMgr::BeginMethod(int, eSCALEFORM_BASE_CLASS, char const*, int)"),
    ("scaleform_add_param_float", "CScaleformMgr::AddParamFloat(double)"),
    // The game's text box (save names, the phone's cheat entry, ...): touch
    // devices show the system keyboard while it is open.
    ("text_box_open", "CTextInputBox::Open(rage::ioVirtualKeyboard::Params const&)"),
    // Its movie goes away however it closes: Close(), or its own update
    // when the text is accepted or cancelled (which does not call Close()).
    ("text_box_close", "CTextInputBox::RemoveTextInputBoxMovie()"),
    // The player's controls, once a frame: they note the device last used
    // (pad or keyboard and mouse), which touch play keeps on the pad.
    ("control_update", "CControl::Update()"),
    // The player on foot, once a frame (touch play shows its controls).
    ("player_on_foot", "CTaskPlayerOnFoot::ProcessPreFSM()"),
    // The vehicle the player would get into: called to show the touch
    // controls' Enter button.
    ("scan_for_vehicle_to_enter", "CPlayerInfo::ScanForVehicleToEnter(CPed*, rage::Vector3 const&, bool)"),
    // The player driving, once a frame, one for each kind of vehicle: touch
    // play shows controls for it.
    ("drive_automobile", "CTaskVehiclePlayerDriveAutomobile::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_bike", "CTaskVehiclePlayerDriveBike::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_boat", "CTaskVehiclePlayerDriveBoat::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_heli", "CTaskVehiclePlayerDriveHeli::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_autogyro", "CTaskVehiclePlayerDriveAutogyro::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_plane", "CTaskVehiclePlayerDrivePlane::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    ("drive_submarine", "CTaskVehiclePlayerDriveSubmarine::ProcessDriverInputsForPlayerOnUpdate(CPed*, CControl*, CVehicle*)"),
    // The player in a vehicle, at any seat, once a frame.
    ("player_in_vehicle", "CTaskPlayerDrive::ProcessPreFSM()"),
    // Any ped's parachute (skydiving too), once a frame.
    ("parachute_update", "CTaskParachute::UpdateFSM(int, rage::aiTask::FSM_Event)"),
    // A cutscene's skip, checked once a frame while one plays (after its
    // first 2 s, until it fades out).
    ("cutscene_update_skip", "CutSceneManager::UpdateSkip()"),
    // Whether the pause menu (the map, the store, ...) is open: called once
    // a frame for the touch controls, which then act as a mouse.
    ("pause_menu_is_active", "CPauseMenu::IsActive(PM_ActiveFlags)"),
    // The pause menu goes a level deeper or back (a column, a list): a
    // direct touch tap that only went in clicks again.
    ("menu_shift_depth", "CPauseMenu::MENU_SHIFT_DEPTH(eMenuceptionDir, bool, bool)"),
    // Called: the pause menu shows its map (direct touch drags it).
    ("pause_menu_in_map", "CPauseMenu::IsInMapScreen()"),
    // Called: the screen is black, or a loading screen shows (nothing to
    // play: the touch controls leave).
    ("screen_faded_out", "camInterface::IsFadedOut()"),
    ("loading_screens_active", "CLoadingScreens::AreActive()"),
    // A script's menu shows the mouse's cursor this frame (it does for
    // keyboard and mouse only): direct touch clicks in it.
    ("set_mouse_cursor_this_frame", "hud_commands::SetupScriptCommands()::scrWrapped_SET_MOUSE_CURSOR_THIS_FRAME::Call(rage::scrThread::Info&)"),
]
