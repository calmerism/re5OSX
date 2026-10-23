// Hooks for the runtime's `granular.rs` (see ../hooks.rs).
&[
    // A granular sound's player for one wave (vehicle engines): an Opus wave
    // is decoded to PCM once, and played from it as on PC.
    ("granular_submix_new", "rage::audGranularSubmix::audGranularSubmix(rage::audWaveRef const*, rage::audGranularSubmix::audGrainData const*, unsigned int)"),
    // Deletes the grain player's submixes: their PCM is released.
    ("grain_player_shutdown", "rage::audGrainPlayer::Shutdown()"),
    // The game's Opus decoder, which the host runs over a whole wave.
    ("opus_init", "rage::audDecoderOpus::Init(rage::audWaveFormat::audStreamFormat, unsigned int, unsigned int)"),
    ("opus_submit", "rage::audDecoderOpus::SubmitPacket(rage::audPacket&)"),
    ("opus_update", "rage::audDecoderOpus::Update()"),
    ("opus_read", "rage::audDecoderOpus::ReadData(short*, unsigned int) const"),
    ("opus_advance", "rage::audDecoderOpus::AdvanceReadPtr(unsigned int)"),
    ("opus_shutdown", "rage::audDecoderOpus::Shutdown()"),
]
