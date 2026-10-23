// Hooks for the runtime's `cloud.rs` (see ../hooks.rs).
&[
    // GTA Online's characters, kept in the user directory: the stats' saves
    // and loads (without -lan's veto), the profile stats, the stats files'
    // server-authoritative values.
    ("xml_bool_attribute", "rage::parElement::FindAttributeBoolValue(char const*, bool, bool) const"),
    ("stats_init", "CStatsSavesMgr::Init(unsigned int)"),
    ("stats_update", "CStatsDataMgr::Update()"),
    ("stats_queue_load", "CStatsSavesMgr::QueueLoad(eStatsLoadType)"),
    ("stats_queue_save", "CStatsSavesMgr::QueueSave(eStatsSaveType, unsigned int)"),
    ("stats_load_failed", "StatsInterface::CloudFileLoadFailed(int)"),
    ("stats_save", "CStatsSavesMgr::Save(eStatsSaveType, unsigned int, eSaveTypes, unsigned int, int)"),
    ("stats_load", "CStatsSavesMgr::Load(eStatsLoadType, int)"),
    ("cloud_available", "NetworkInterface::IsCloudAvailable(int)"),
    ("profile_stats_failed", "CProfileStats::SynchronizedFailed(CProfileStats::eSyncType, int&) const"),
    ("profile_stats_synced", "CProfileStats::Synchronized(CProfileStats::eSyncType) const"),
    ("mpstats_load_update", "CSavegameQueuedOperation_MPStats_Load::Update()"),
    ("mpstats_save_update", "CSavegameQueuedOperation_MPStats_Save::Update()"),
    ("ros_credentials_valid", "rage::rlRosCredentials::IsValid() const"),
    // Rockstar's cloud files, on the disk; the engine's grow buffers, which
    // they are read into (files.rs).
    ("post_member_file", "rage::rlCloud::PostMemberFile(int, rage::rlCloudOnlineService, char const*, void const*, unsigned int, rage::rlCloud::PostType, rage::rlCloudFileInfo*, rage::sysMemAllocator*, rage::netStatus*)"),
    ("get_member_file", "rage::rlCloud::GetMemberFile(int, rage::rlCloudMemberId const&, rage::rlCloudOnlineService, char const*, rage::rlRosSecurityFlags, unsigned long, rage::netHttpOptions, rage::fiDevice const*, fiHandle__*, rage::rlCloudFileInfo*, rage::sysMemAllocator*, rage::netStatus*)"),
    ("grow_buffer_append", "rage::datGrowBuffer::Append(void const*, unsigned int)"),
]
