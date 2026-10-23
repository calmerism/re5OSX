// Hooks for the runtime's `online.rs` (see ../hooks.rs).
&[
    // A signed-in local gamer: Social Club's stand-ins go in at the live
    // manager's start; its update keeps the network up (vnet.rs).
    ("rl_gamer_handle", "rage::internal::GetGamerHandle(int, rage::rlGamerHandle*)"),
    ("rl_offline_name", "rage::internal::GetOfflineName(int, char*, unsigned int)"),
    ("rl_pc_initialized", "rage::rlPc::IsInitialized()"),
    ("live_manager_init", "CLiveManager::Init(unsigned int)"),
    ("live_manager_update", "CLiveManager::Update(unsigned int)"),
    // Sessions hosted (with their kind: public, invite only, friends, solo)
    // and looked for: the lobby they are in.
    ("session_host_config", "CNetworkSession::HostSession(NetworkGameConfig&, unsigned int)"),
    ("freeroam_matchmaking", "CNetworkSession::DoFreeroamMatchmaking(int, int, unsigned int, unsigned int)"),
    ("friend_matchmaking", "CNetworkSession::DoFriendMatchmaking(int, int, unsigned int, unsigned int)"),
    // Called once: the network's settings from the tunables (their defaults).
    ("network_tunables_read", "CNetworkTunablesListener::OnTunablesRead()"),
    ("presence_online", "rage::rlPresence::IsOnline(int)"),
    // The checks on the way into freemode that Rockstar's services would
    // pass: the hardware bail, privileges, the prologue, the store, the
    // network features, the mugshot photo.
    ("bail_on_multiplayer", "CLiveManager::BailOnMultiplayer(eBailReason, int)"),
    // Sessions hosted and joined on Rockstar's network (a job's transition
    // session is, whatever the game's mode): on the lobby's LAN instead.
    ("session_host", "rage::snSession::Host(int, rage::rlNetworkMode, int, int, rage::rlMatchingAttributes const&, unsigned int, void const*, unsigned int, rage::netStatus*)"),
    ("session_join", "rage::snSession::Join(int, rage::rlSessionInfo const&, rage::rlNetworkMode, rage::rlSlotType, unsigned int, void const*, unsigned int, rage::rlGamerHandle const*, unsigned int, rage::netStatus*)"),
    ("session_find", "rage::rlSessionFinder::Find(int, rage::rlNetworkMode, unsigned int, int, rage::rlMatchingFilter const&, rage::rlSessionDetail*, unsigned int, unsigned int, unsigned int*, rage::netStatus*)"),
    ("session_query", "rage::rlSessionManager::QueryDetail(rage::rlNetworkMode, unsigned int, unsigned int, unsigned int, unsigned int, bool, rage::rlSessionInfo const*, unsigned int, rage::rlGamerHandle, rage::rlSessionDetail*, unsigned int*, rage::netStatus*)"),
    ("session_query_any", "rage::rlSessionManager::QueryDetail(rage::rlNetworkMode, unsigned int, unsigned int, unsigned int, unsigned int, bool, rage::rlSessionInfo const*, unsigned int, rage::rlSessionDetail*, unsigned int*, rage::netStatus*)"),
    ("ros_privilege", "rage::rlRosCredentials::HasPrivilege(rage::rlRosPrivilegeId) const"),
    ("ros_privilege_end_date", "rage::rlRosCredentials::HasPrivilegeEndDate(rage::rlRosPrivilegeId, bool*, unsigned long*) const"),
    ("profile_setting_int", "CProfileSettings::GetInt(CProfileSettings::ProfileSettingId)"),
    ("can_access_network_features", "CNetwork::CanAccessNetworkFeatures(eNetworkAccessArea, eNetworkAccessCode*)"),
    ("can_enter_multiplayer", "CNetworkSession::CanEnterMultiplayer()"),
    ("populate_store_data", "cCommerceManager::PopulateThinData(bool)"),
    ("photo_to_jpeg", "CHighQualityScreenshot::StoreScreenShotInBuffer(unsigned char*, unsigned int*, unsigned int, void*, int, int, int, unsigned int, CPhotoBuffer*)"),
    // The scripts: their bytecode patches, and natives answering about
    // Rockstar's services or content the game data lacks.
    ("script_program_loaded", "rage::scrProgram::scrProgram(rage::datResource&)"),
    ("profanity_valid", "socialclub_commands::SetupScriptCommands()::scrWrapped_SC_PROFANITY_GET_CHECK_IS_VALID::Call(rage::scrThread::Info&)"),
    ("profanity_pending", "socialclub_commands::SetupScriptCommands()::scrWrapped_SC_PROFANITY_GET_CHECK_IS_PENDING::Call(rage::scrThread::Info&)"),
    ("profanity_status", "socialclub_commands::SetupScriptCommands()::scrWrapped_SC_PROFANITY_GET_STRING_STATUS::Call(rage::scrThread::Info&)"),
    ("social_club_account", "network_commands::SetupScriptCommands()::scrWrapped_NETWORK_HAS_SOCIAL_CLUB_ACCOUNT::Call(rage::scrThread::Info&)"),
    ("online_policies_current", "hud_commands::SetupScriptCommands()::scrWrapped_ARE_ONLINE_POLICIES_UP_TO_DATE::Call(rage::scrThread::Info&)"),
    ("match_history_id", "stats_commands::SetupScriptCommands()::scrWrapped_PLAYSTATS_CREATE_MATCH_HISTORY_ID_2::Call(rage::scrThread::Info&)"),
    ("savemigration_mp_enabled", "savemigration_commands::SetupScriptCommands()::scrWrapped_SAVEMIGRATION_IS_MP_ENABLED::Call(rage::scrThread::Info&)"),
    ("bad_pack_order", "dlc_commands::SetupScriptCommands()::scrWrapped_GET_EVER_HAD_BAD_PACK_ORDER::Call(rage::scrThread::Info&)"),
]
