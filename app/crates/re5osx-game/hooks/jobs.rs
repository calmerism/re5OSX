// Hooks for the runtime's `jobs.rs` (see ../hooks.rs).
&[
    // GTA Online's jobs (races, deathmatches, missions...): the queries for
    // them and their files, answered from the disk instead of Rockstar's
    // servers.
    ("ugc_query_content", "rage::rlUgc::QueryContent(int, rage::rlUgcContentType, char const*, char const*, int, int, int*, int*, rage::atDelegate<void (int, char const*, rage::rlUgcMetadata const*, rage::rlUgcRatings const*, char const*, unsigned int, unsigned int, rage::rlUgcPlayer const*)> const&, rage::sysMemAllocator*, rage::netStatus*)"),
    ("ugc_get_file", "rage::netCloudRequestGetUgcFile::DoRequest()"),
    // Which waits for Rockstar's credentials before it starts (the jobs'
    // and the characters' files share it).
    ("ugc_can_start", "rage::netCloudRequestGetMemberFile::CanStartRequest() const"),
    // The Creator's saves: new jobs, saved again, published.
    ("ugc_create", "CUserContentManager::Create(char const*, char const*, rage::rlUgc::SourceFile const*, unsigned int, char const*, char const*, rage::rlUgcContentType, bool)"),
    ("ugc_update", "CUserContentManager::UpdateContent(char const*, char const*, char const*, char const*, rage::rlUgc::SourceFile const*, unsigned int, char const*, rage::rlUgcContentType)"),
    ("ugc_publish", "CUserContentManager::Publish(char const*, char const*, rage::rlUgcContentType, char const*)"),
    // Called to hand a query's results over, as Rockstar's answer would.
    ("ugc_metadata_new", "rage::rlUgcMetadata::rlUgcMetadata()"),
    ("ugc_result", "rage::atDelegate8<void, int, char const*, rage::rlUgcMetadata const*, rage::rlUgcRatings const*, char const*, unsigned int, unsigned int, rage::rlUgcPlayer const*>::Invoke(int, char const*, rage::rlUgcMetadata const*, rage::rlUgcRatings const*, char const*, unsigned int, unsigned int, rage::rlUgcPlayer const*) const"),
    ("grow_buffer_clear", "rage::datGrowBuffer::Clear()"),
]
