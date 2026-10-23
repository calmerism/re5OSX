//! GTA Online without Rockstar's services: the game plays on a lobby of
//! players (p2p.rs) as on a LAN (vnet.rs), and keeps its characters on the
//! disk (cloud.rs).
//!
//! The engine signs in through Social Club, whose DLL it talks to: `rlPc`
//! keeps a pointer to each of the DLL's interfaces, and the host puts
//! objects there whose vtables lead to host functions, so the engine sees
//! a signed-in, offline player. `rlPc::IsInitialized()` still says no, so
//! what asks it (saves, control settings) keeps its path without Social
//! Club; only code reading the pointers directly (presence) meets the
//! stand-ins, whose methods do nothing but sign the player in.
//!
//! The rest are the checks on the way into freemode that Rockstar's
//! services would pass, passed here.

use std::ffi::c_void;
use std::sync::OnceLock;

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// `rlPc` in this build: Social Club's interface pointers are at +88 to
/// +208, the profile manager's at +96.
const RL_PC: u64 = 15211472;
const INTERFACES: std::ops::Range<u64> = 88..216;
const PROFILE_MANAGER: u64 = 96;
/// Methods in a stand-in's vtable, more than any interface has.
const METHODS: u64 = 64;

/// The engine's switches for GTA Online without Rockstar's services: LAN
/// sessions (the lobby is the LAN, vnet.rs), straight into freemode, no
/// cloud files, catalog, shop transactions or online policies to wait for,
/// sessions for everyone whatever their aim (the game keeps players aiming
/// freely, with a mouse, apart from assisted ones, as touch play on a phone
/// is: they never met), and the story's prologue taken as played (joining a
/// friend's session asks for it: "GTA Online will be available once you
/// have completed the prologue", and the game answered the friend's
/// session every frame).
pub(crate) const ENGINE_ARGS: &[&str] = &[
    "-lan",
    "-StraightIntoFreemode",
    "-netSessionIgnoreAim",
    "-netSpPrologueComplete",
    "-mpavailable",
    "-netMpSkipCloudFileRequirements",
    "-sc_noOnlinePolicy",
    "-netshoptransactionnull",
    "-netGameTransactionsNoToken",
    "-nocatalog",
];


/// The local gamer's id (as Rockstar's ids): made up once per user
/// directory, kept in it.
pub(crate) fn gamer_id() -> u64 {
    gamer_id_in(&crate::rt().config.user_dir)
}

pub(crate) fn gamer_id_in(user_dir: &std::path::Path) -> u64 {
    static ID: OnceLock<u64> = OnceLock::new();
    *ID.get_or_init(|| {
        let path = user_dir.join("online-id");
        if let Some(id) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()) {
            return id;
        }
        use std::hash::{BuildHasher, Hasher};
        // Positive 32-bit: the scripts keep ids in ints.
        let id = std::collections::hash_map::RandomState::new().build_hasher().finish() % 0x7fff_0000 + 0x1_0000;
        if let Err(e) = std::fs::write(&path, id.to_string()) {
            log::warn!("online: keeping the gamer id in {}: {e}", path.display());
        }
        id
    })
}

fn gamer_name() -> String {
    crate::rt().config.online.as_ref().map_or_else(String::new, |o| o.name.clone())
}

/// Sets the game up for GTA Online: the hooks of this module and the
/// others', the stand-ins' methods, the lobby. Before installing the game
/// module.
pub(crate) fn start(online: &crate::Online, hooks: &mut re5osx_game::Hooks) {
    self::hooks(hooks);
    crate::vnet::hooks(hooks);
    crate::cloud::hooks(hooks);
    crate::jobs::hooks(hooks);
    crate::friends::game::hooks(hooks);
    crate::texts::hooks(hooks);
    crate::stats::hooks(hooks);
    // The stand-ins' methods, host functions the game calls through their
    // vtables (`re5osx_game::host_function`): QueryInterface, a method doing
    // nothing (0: S_OK, false, none), the profile manager's IsSignedIn and
    // GetSignInInfo.
    static FNS: OnceLock<[usize; 4]> = OnceLock::new();
    re5osx_game::set_host_functions(FNS.get_or_init(|| {
        [
            query_interface as *const () as usize,
            nothing as *const () as usize,
            signed_in as *const () as usize,
            sign_in_info as *const () as usize,
        ]
    }));
    match crate::p2p::start() {
        Ok(p2p) => p2p.join_lobby(&online.lobby),
        Err(e) => log::error!("p2p: {e}"),
    }
}

const QUERY_INTERFACE: u32 = 0;
const NOTHING: u32 = 1;
const SIGNED_IN: u32 = 2;
const SIGN_IN_INFO: u32 = 3;

// Host functions: called with the thread's instance first, not its `Env`
// (see `re5osx_game::set_host_functions`); the wasm memory is the runtime's.

/// QueryInterface(iid, void** out): no other interface is offered.
unsafe extern "C" fn query_interface(_: *mut c_void, _this: u64, _iid: u64, out: u64) -> u64 {
    if out != 0 {
        crate::rt().memory.mem().set_u64(out, 0);
    }
    0x8000_4002 // E_NOINTERFACE
}

unsafe extern "C" fn nothing(_: *mut c_void) -> u64 {
    0
}

unsafe extern "C" fn signed_in(_: *mut c_void) -> u64 {
    1
}

/// GetSignInInfo(IProfile*): { vtable*, s64 id, char nickname[17] }.
unsafe extern "C" fn sign_in_info(_: *mut c_void, _this: u64, profile: u64) -> u64 {
    let m = crate::rt().memory.mem();
    m.set_u64(profile + 8, gamer_id());
    m.write_str(profile + 16, &gamer_name(), 17);
    0
}

/// Puts the stand-ins in `rlPc`: one object for the profile manager, one
/// for all the other interfaces.
unsafe fn install(env: *mut Env) {
    let m = unsafe { Mem::of(env) };
    let object = |methods: &[u32]| {
        let vtable = unsafe { orig::builtin_malloc(env, METHODS * 8) };
        for i in 0..METHODS {
            let f = methods.get(i as usize).copied().unwrap_or(NOTHING);
            m.set_u64(vtable + i * 8, re5osx_game::host_function(f));
        }
        let object = unsafe { orig::builtin_malloc(env, 8) };
        m.set_u64(object, vtable);
        object
    };
    // IsSignedIn: yes; IsOnline: no (Social Club's own online paths stay off).
    let profile_manager = object(&[QUERY_INTERFACE, SIGNED_IN, NOTHING, SIGN_IN_INFO]);
    let other = object(&[QUERY_INTERFACE]);
    for offset in INTERFACES.step_by(8) {
        m.set_u64(RL_PC + offset, if offset == PROFILE_MANAGER { profile_manager } else { other });
    }
}

/// Hook for `CLiveManager::Init(unsigned int)`: the stand-ins go in before
/// the live manager first asks who is signed in.
pub(crate) unsafe extern "C" fn live_manager_init(env: *mut Env, mode: u32) {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| unsafe { install(env) });
    unsafe { orig::live_manager_init(env, mode) };
    // Init clears the store's state; nothing would populate it again.
    unsafe { populate_store_data(env) };
}

/// Hook for `CLiveManager::Update(unsigned int)`: the device's network
/// stays up (see `vnet::network_up`), the tunables' request is over;
/// friends (friends/game.rs).
pub(crate) unsafe extern "C" fn live_manager_update(env: *mut Env, mode: u32) {
    unsafe { crate::friends::game::update(env) };
    crate::vnet::network_up(unsafe { Mem::of(env) });
    unsafe { tunables_requested(env) };
    unsafe { orig::live_manager_update(env, mode) };
    crate::vnet::network_up(unsafe { Mem::of(env) });
}

/// Rockstar's cloud tunables never come: their request counts as over
/// (`CTunables`' flag, set at the end of `OnCloudEvent` either way), as
/// what waits for it (invites, CanEnterMultiplayer) asks, and the network's
/// settings are read from them as they would be then: their defaults (an
/// invite's lifetime among them, else 0: gone as it comes). The scripts
/// keep their defaults too.
unsafe fn tunables_requested(env: *mut Env) {
    const TUNABLES: u64 = 19275808;
    const CLOUD_REQUEST_DONE: u64 = 464;
    let m = unsafe { Mem::of(env) };
    let tunables = m.u64(TUNABLES);
    if tunables != 0 && m.u8(tunables + CLOUD_REQUEST_DONE) == 0 {
        // Read before the flag is set: once set, the reader would also ask
        // Rockstar for its content's URLs.
        unsafe { orig::network_tunables_read(env, 0) };
        m.set_u8(tunables + CLOUD_REQUEST_DONE, 1);
        log::info!("online: the network's tunables, their defaults");
    }
}

/// `CNetworkSession`'s host flags (its `LogHostFlags`).
const HF_IS_PRIVATE: u32 = 1;
const HF_SINGLEPLAYER_ONLY: u32 = 4;
const HF_CLOSED: u32 = 8;
const HF_CLOSED_FRIENDS: u32 = 16;
const HF_CLOSED_CREW: u32 = 32;

/// Hook for `CNetworkSession::HostSession(NetworkGameConfig&, unsigned
/// int)`, which every session the game hosts goes through (a job's
/// excepted): the lobby it is in follows its kind, as the pause menu's
/// sessions are. A public one in everyone's; the others (invite only,
/// crew, closed friends, solo) in a new private lobby, whose key is the
/// only way in: friends get it with an invite, or from the presence for a
/// friends' session; nobody for a solo one.
pub(crate) unsafe extern "C" fn session_host_config(env: *mut Env, session: u64, config: u64, flags: u32) -> u32 {
    use crate::friends::SessionKind;
    let kind = if flags & HF_SINGLEPLAYER_ONLY != 0 {
        SessionKind::Solo
    } else if flags & HF_CLOSED_FRIENDS != 0 {
        SessionKind::Friends
    } else if flags & (HF_IS_PRIVATE | HF_CLOSED | HF_CLOSED_CREW) != 0 {
        SessionKind::Closed
    } else {
        SessionKind::Public
    };
    in_lobby_for(kind);
    unsafe { orig::session_host_config(env, session, config, flags) }
}

/// Hook for `CNetworkSession::DoFreeroamMatchmaking(int, int, unsigned int,
/// unsigned int)`: looking for a public session, in everyone's lobby.
pub(crate) unsafe extern "C" fn freeroam_matchmaking(env: *mut Env, session: u64, mode: u32, max: u32, flags: u32) -> u32 {
    in_lobby_for(crate::friends::SessionKind::Public);
    unsafe { orig::freeroam_matchmaking(env, session, mode, max, flags) }
}

/// Hook for `CNetworkSession::DoFriendMatchmaking(int, int, unsigned int,
/// unsigned int)`: "Join Friends", into a friend's session, found in the
/// friends' presence (friends/game.rs): the friend's lobby is the
/// friend's to share.
pub(crate) unsafe extern "C" fn friend_matchmaking(env: *mut Env, session: u64, mode: u32, max: u32, flags: u32) -> u32 {
    crate::friends::find_friends();
    unsafe { orig::friend_matchmaking(env, session, mode, max, flags) }
}

/// Moves the player to the lobby for a session of `kind`: everyone's (the
/// friend's, while one is followed there), or a new private one.
fn in_lobby_for(kind: crate::friends::SessionKind) {
    crate::friends::set_session_kind(kind);
    let Some(p2p) = crate::p2p::get() else { return };
    if kind == crate::friends::SessionKind::Public {
        if crate::friends::following().is_none() {
            p2p.switch_lobby("");
        }
    } else {
        // Its key, the way in: 128 bits from the system (as long a name as
        // friends take).
        let mut key = [0u8; 16];
        if let Err(e) = getrandom::fill(&mut key) {
            // (In a hook: no panic. The lobby stays as it is.)
            log::error!("online: no random numbers for a private lobby: {e}");
            return;
        }
        p2p.switch_lobby(&key.iter().map(|b| format!("{b:02x}")).collect::<String>());
    }
}

/// Hook for `CLiveManager::BailOnMultiplayer(eBailReason, int)`: the
/// browser build has no network hardware (`netHardware::IsAvailable()` is
/// false there, folded into the callers), so every session ends at once
/// "because the hardware went offline". That one bail is ignored.
pub(crate) unsafe extern "C" fn bail_on_multiplayer(env: *mut Env, reason: u32, context: u32) {
    const BAIL_PROFILE_CHANGE: u32 = 2;
    const BAIL_CTX_PROFILE_CHANGE_HARDWARE_OFFLINE: u32 = 0x3001;
    if reason == BAIL_PROFILE_CHANGE && context == BAIL_CTX_PROFILE_CHANGE_HARDWARE_OFFLINE {
        return;
    }
    unsafe { orig::bail_on_multiplayer(env, reason, context) }
}

/// Hook for `rage::rlPc::IsInitialized()`: no, with the stand-ins in (see
/// the module's notes).
pub(crate) unsafe extern "C" fn rl_pc_initialized(_: *mut Env) -> u32 {
    0
}

/// Privileges Rockstar's servers would grant (`rlRosPrivilegeId` bits): all
/// the game's defaults but 7 (banned), 12 (Rockstar developer) and 20
/// (cheater).
const PRIVILEGES: u32 = 0b0110_1111_1110_1111_0111_1111;

/// Hook for `rage::rlRosCredentials::HasPrivilege(rlRosPrivilegeId)`: the
/// privileges, without credentials (valid ones would send the engine to
/// Rockstar's web services).
pub(crate) unsafe extern "C" fn ros_privilege(_env: *mut Env, _creds: u64, id: u32) -> u32 {
    (id < 32 && PRIVILEGES >> id & 1 != 0) as u32
}

/// Hook for `rage::rlRosCredentials::HasPrivilegeEndDate(rlRosPrivilegeId,
/// bool*, unsigned long*)`: no privilege ends. Without credentials the
/// engine answers the same after an error line, and CNetwork asks every
/// frame.
pub(crate) unsafe extern "C" fn ros_privilege_end_date(
    _env: *mut Env,
    _creds: u64,
    _id: u32,
    _ends: u64,
    _date: u64,
) -> u32 {
    0
}

/// The profile setting that says the story's prologue is done, which GTA
/// Online asks for.
const PROLOGUE_COMPLETE: u32 = 903;

/// Hook for `CProfileSettings::GetInt(ProfileSettingId)`.
pub(crate) unsafe extern "C" fn profile_setting_int(env: *mut Env, this: u64, id: u32) -> u32 {
    if id == PROLOGUE_COMPLETE { 1 } else { unsafe { orig::profile_setting_int(env, this, id) } }
}

/// Hooks for natives whose answer depends on Rockstar's servers or on
/// content this game data lacks: the result scripts get here.
macro_rules! native_results {
    ($($(#[$doc:meta])* $hook:ident = $value:expr;)*) => {$(
        $(#[$doc])*
        pub(crate) unsafe extern "C" fn $hook(env: *mut Env, info: u64) {
            unsafe { orig::$hook(env, info) };
            let m = unsafe { Mem::of(env) };
            m.set_u32(m.u64(info), $value);
        }
    )*};
}

native_results! {
    /// `GET_EVER_HAD_BAD_PACK_ORDER`: the game data has the 2014 DLC packs
    /// only, the scripts expect later ones.
    bad_pack_order = 0;
    /// `SAVEMIGRATION_IS_MP_ENABLED`: no character to bring over from the
    /// consoles (the servers' service).
    savemigration_mp_enabled = 0;
    /// `SC_PROFANITY_GET_STRING_STATUS`: passed.
    profanity_status = 0;
    /// `SC_PROFANITY_GET_CHECK_IS_PENDING`: never waiting on it.
    profanity_pending = 0;
    /// `SC_PROFANITY_GET_CHECK_IS_VALID`: Social Club's text check (names); every text passes.
    profanity_valid = 1;
    /// `NETWORK_HAS_SOCIAL_CLUB_ACCOUNT`: the player has one (the pause
    /// menu's Creator needs it).
    social_club_account = 1;
    /// `ARE_ONLINE_POLICIES_UP_TO_DATE`: there are no Rockstar policies to
    /// accept (`-sc_noOnlinePolicy`); an invite is dropped otherwise.
    online_policies_current = 1;
}

/// Hook for `PLAYSTATS_CREATE_MATCH_HISTORY_ID_2(int&, int&)`: a match's id,
/// the Rockstar account's and the time, or the network card's address
/// (which the engine cannot read here); the minigames' host (golf) waits
/// for one. The player's id and the time.
pub(crate) unsafe extern "C" fn match_history_id(env: *mut Env, info: u64) {
    let m = unsafe { Mem::of(env) };
    let params = m.u64(info + 16);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    m.set_u32(m.u64(params), gamer_id() as u32);
    m.set_u32(m.u64(params + 8), now as u32);
    m.set_u32(m.u64(info), 1);
}

/// Hook for `CNetwork::CanAccessNetworkFeatures(eNetworkAccessArea,
/// eNetworkAccessCode*)`: the gate of every online feature (multiplayer,
/// creators, store...), whose checks are about Rockstar's services; granted.
pub(crate) unsafe extern "C" fn can_access_network_features(env: *mut Env, _area: u32, code: u64) -> u32 {
    if code != 0 {
        unsafe { Mem::of(env) }.set_u32(code, 0); // Access_Granted
    }
    1
}

/// Hook for `rage::rlPresence::IsOnline(int)`: the local gamer counts as
/// online (sessions and the privileges need it); the stand-in profile
/// manager still says offline, so Social Club's own online paths stay off.
pub(crate) unsafe extern "C" fn presence_online(env: *mut Env, index: u32) -> u32 {
    if index == 0 { 1 } else { unsafe { orig::presence_online(env, index) } }
}

/// Hook for `cCommerceManager::PopulateThinData(bool)` (the argument is
/// gone from the compiled function): the store's catalogue comes from
/// Rockstar's servers, and while it loads it holds the network memory
/// sessions need (CanEnterMultiplayer, matchmaking, hosting). Here it is
/// done at once, empty: the state the engine sets when a download ends.
pub(crate) unsafe extern "C" fn populate_store_data(env: *mut Env) {
    const STATE: u64 = 29308996; // 2: idle
    const POPULATED: u64 = 29309000; // u8 populated, u8 in progress
    const USE_CACHE: u64 = 29309002;
    let m = unsafe { Mem::of(env) };
    m.set_u8(USE_CACHE, 0);
    m.set_u32(STATE, 2);
    m.set_u16(POPULATED, 1);
}

/// Hook for `CHighQualityScreenshot::StoreScreenShotInBuffer(...)` (one
/// argument is gone from the compiled function): the character's mugshot
/// ends the process in libjpeg's error exit here. It fails instead, which
/// the game takes (the character has no photo).
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe extern "C" fn photo_to_jpeg(
    env: *mut Env,
    _out: u64,
    size: u64,
    _max: u32,
    _pixels: u64,
    _width: u32,
    _height: u32,
    _pitch: u32,
    _photo: u64,
) -> u32 {
    unsafe { Mem::of(env) }.set_u32(size, 0);
    0
}

/// Hook for `CNetworkSession::CanEnterMultiplayer()`: with `-lan` the
/// engine counts the ROS credentials as valid and then waits for Rockstar's
/// cloud tunables, which never come. The other conditions stand: the network
/// memory free (the store's data, see `populate_store_data`; no save
/// queued, which the engine asks only of story saves) and the session idle. Then the lobby's players are waited for
/// (`P2p::may_enter`).
pub(crate) unsafe extern "C" fn can_enter_multiplayer(env: *mut Env, session: u64) -> u32 {
    const STATE: u64 = 99916;
    const STORE: u64 = 29309000; // populated, in progress
    const SAVE_QUEUE: u64 = 23275800;
    let m = unsafe { Mem::of(env) };
    let can = unsafe { orig::can_enter_multiplayer(env, session) } != 0 || {
        let store_ready = m.u8(STORE) & 1 != 0 && m.u8(STORE + 1) == 0;
        store_ready && m.u32(SAVE_QUEUE) == 0 && m.u32(session + STATE) == 0
    };
    (can && crate::p2p::get().is_none_or(|p| p.may_enter())) as u32
}

/// Changes to the game's scripts (bytecode), made as each program loads:
/// (script, code offset, the bytes there, the bytes put instead). A patch
/// whose bytes differ is left out and logged.
const SCRIPT_PATCHES: &[(&str, u32, &[u8], &[u8])] = &[
    // g_bFM_Rank_Unlocks_ON = FALSE: GTA Online's activities (golf, tennis,
    // darts, arm wrestling, the flight school, survivals...) are open from
    // the start instead of at a rank and after a contact's call, as the
    // game's own scripts allow; with Rockstar's jobs gone, ranks come
    // slowly here.
    ("MP_FM_registration", 194, &[OP_PUSH_1, 99, 72, 7, 28], &[OP_PUSH_0, 99, 72, 7, 28]),
    // CAN_ENTER_MP_AFTER_VALID_CHARACTER_CREATOR_FLOW: with no character yet,
    // the straight-into-freemode start (LOBBY_AUTO_MULTIPLAYER_FREEMODE)
    // must not keep the player from the character creator.
    ("MainTransition", 2510677, &[0x2c, 0x01, 0x03, 0xab], &[OP_PUSH_0, OP_NOP, OP_NOP, OP_NOP]),
    // SHOULD_RUN_ROCKSTAR_WELCOME_SCREEN, -sc_noOnlinePolicy: the welcome
    // screen (Rockstar's policies) is skipped, but it is what takes the
    // loading screen down, and the pause menu (the character creator) does
    // not run under one. Its debug print becomes SHUTDOWN_LOADING_SCREEN.
    (
        "MainTransition",
        2504003,
        &[0x73, 0x64, 0x7c, 0xde, 0x12, 0x66, 0x2c, 0x08, 0x03, 0x79],
        &[0x2c, 0x00, 0x00, 0xf9, OP_NOP, OP_NOP, OP_NOP, OP_NOP, OP_NOP, OP_NOP],
    ),
];
const OP_NOP: u8 = 0;
const OP_PUSH_0: u8 = 113;
const OP_PUSH_1: u8 = 114;
/// Script code is kept in pages of this size.
const SCRIPT_PAGE: u32 = 0x4000;

/// Hook for `rage::scrProgram::scrProgram(rage::datResource&)`: a script
/// program has loaded; the patches for it go in.
pub(crate) unsafe extern "C" fn script_program_loaded(env: *mut Env, this: u64, rsc: u64) -> u64 {
    let program = unsafe { orig::script_program_loaded(env, this, rsc) };
    let m = unsafe { Mem::of(env) };
    let name = m.str(m.u64(this + 96));
    let (pages, size) = (m.u64(this + 16), m.u32(this + 28));
    for &(script, at, old, new) in SCRIPT_PATCHES.iter().filter(|p| p.0 == name) {
        if at + old.len() as u32 > size || at % SCRIPT_PAGE + old.len() as u32 > SCRIPT_PAGE {
            log::warn!("online: script patch {script}+{at} is out of the code");
            continue;
        }
        let code = m.u64(pages + u64::from(at / SCRIPT_PAGE) * 8) + u64::from(at % SCRIPT_PAGE);
        let bytes = m.bytes_mut(code, old.len());
        if bytes != old {
            log::warn!("online: script patch {script}+{at} skipped: found {bytes:02x?}");
            continue;
        }
        bytes.copy_from_slice(new);
        log::info!("online: script patch {script}+{at}");
    }
    program
}

/// gameconfig.xml: the scripts are a later version than the game data, and
/// they register more player broadcast data blocks than it provides; the
/// engine quits when a script finds none free ("No local player broadcast
/// data handler"). Freemode alone fills the larger sizes, and with the shops
/// and ambient scripts around the player all 31 were in use (an iPhone log
/// listed them), so the two largest sizes and the three smallest get more
/// instances. A smaller block takes a larger one when its size has none
/// left. Each longer number costs a tab of its line's indentation, so the
/// file keeps its length.
pub(crate) fn more_broadcast_data(text: String) -> Option<String> {
    const FIELD: &str = "<NumInstances value=\"";
    let start = text.find("<PlayerBroadcastData>")?;
    let end = start + text[start..].find("</PlayerBroadcastData>")?;
    let mut block = text[start..end].to_string();
    for (size, instances) in [(3600, 3), (2500, 4), (150, 12), (80, 12), (40, 16)] {
        let at = block.find(&format!("<SizeOfData value=\"{size}\"/>"))?;
        let field = at + block[at..].find(FIELD)?;
        let digits = field + FIELD.len()..field + FIELD.len() + block[field + FIELD.len()..].find('"')?;
        let new = instances.to_string();
        let longer = new.len().checked_sub(digits.len())?;
        if !block[..field].ends_with(&"\t".repeat(longer)) {
            return None;
        }
        block.replace_range(digits, &new);
        block.drain(field - longer..field);
    }
    Some(format!("{}{}{}", &text[..start], block, &text[end..]))
}

/// Hook for `rage::internal::GetGamerHandle(int, rlGamerHandle*)` (the index
/// is gone from the compiled function): a Social Club handle, { u64 id, u8
/// service = 3, u8 local index }.
pub(crate) unsafe extern "C" fn gamer_handle(env: *mut Env, handle: u64) -> u32 {
    let m = unsafe { Mem::of(env) };
    // All 16 bytes, as the engine's: invites send them whole to friends.
    m.bytes_mut(handle, 16).fill(0);
    m.set_u64(handle, gamer_id());
    m.set_u8(handle + 8, 3);
    1
}

/// Hook for `rage::internal::GetOfflineName(int, char*, unsigned int)`.
pub(crate) unsafe extern "C" fn offline_name(env: *mut Env, name: u64) -> u32 {
    unsafe { Mem::of(env) }.write_str(name, &gamer_name(), 17);
    1
}

/// `rlNetworkMode`s: Rockstar's network, the LAN (the lobby, vnet.rs).
const NETWORK_ONLINE: u32 = 1;
const NETWORK_LAN: u32 = 2;

/// Rockstar's network is the lobby's LAN here: the transition sessions
/// that jobs are played in are hosted and joined online whatever the
/// game's mode (`CNetworkSession::HostTransition`).
fn on_lan(mode: u32) -> u32 {
    if mode == NETWORK_ONLINE { NETWORK_LAN } else { mode }
}

/// Hooks for `rage::snSession::Host(int, rlNetworkMode, ...)` and
/// `Join(int, rlSessionInfo const&, rlNetworkMode, ...)`: on the LAN.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe extern "C" fn session_host(
    env: *mut Env,
    session: u64,
    gamer: u32,
    mode: u32,
    public_slots: u32,
    private_slots: u32,
    attributes: u64,
    flags: u32,
    data: u64,
    size: u32,
    status: u64,
) -> u32 {
    let mode = on_lan(mode);
    unsafe { orig::session_host(env, session, gamer, mode, public_slots, private_slots, attributes, flags, data, size, status) }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe extern "C" fn session_join(
    env: *mut Env,
    session: u64,
    gamer: u32,
    info: u64,
    mode: u32,
    slot: u32,
    flags: u32,
    data: u64,
    size: u32,
    group: u64,
    group_size: u32,
    status: u64,
) -> u32 {
    let mode = on_lan(mode);
    unsafe { orig::session_join(env, session, gamer, info, mode, slot, flags, data, size, group, group_size, status) }
}

/// Hooks for `rage::rlSessionFinder::Find(int, rlNetworkMode, ...)` and
/// the two `rlSessionManager::QueryDetail(rlNetworkMode, ...)`: finding
/// and asking about sessions (a job's, joined or looked for), on the LAN.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe extern "C" fn session_find(
    env: *mut Env,
    gamer: u32,
    mode: u32,
    a: u32,
    b: u32,
    filter: u64,
    details: u64,
    c: u32,
    found: u64,
    status: u64,
) -> u32 {
    unsafe { orig::session_find(env, gamer, on_lan(mode), a, b, filter, details, c, found, status) }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe extern "C" fn session_query(
    env: *mut Env,
    mode: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    infos: u64,
    count: u32,
    gamer: u64,
    details: u64,
    found: u64,
    status: u64,
) -> u32 {
    unsafe { orig::session_query(env, on_lan(mode), a, b, c, d, e, infos, count, gamer, details, found, status) }
}

pub(crate) unsafe extern "C" fn session_query_any(env: *mut Env, mode: u32, a: u32, b: u64, c: u64, d: u64, e: u64) -> u32 {
    unsafe { orig::session_query_any(env, on_lan(mode), a, b, c, d, e) }
}

/// This module's hooks, online.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.session_host = Some(session_host);
    hooks.session_join = Some(session_join);
    hooks.session_find = Some(session_find);
    hooks.session_query = Some(session_query);
    hooks.session_query_any = Some(session_query_any);
    hooks.rl_gamer_handle = Some(gamer_handle);
    hooks.rl_offline_name = Some(offline_name);
    hooks.live_manager_init = Some(live_manager_init);
    hooks.live_manager_update = Some(live_manager_update);
    hooks.session_host_config = Some(session_host_config);
    hooks.freeroam_matchmaking = Some(freeroam_matchmaking);
    hooks.friend_matchmaking = Some(friend_matchmaking);
    hooks.bail_on_multiplayer = Some(bail_on_multiplayer);
    hooks.rl_pc_initialized = Some(rl_pc_initialized);
    hooks.ros_privilege = Some(ros_privilege);
    hooks.ros_privilege_end_date = Some(ros_privilege_end_date);
    hooks.profile_setting_int = Some(profile_setting_int);
    hooks.bad_pack_order = Some(bad_pack_order);
    hooks.savemigration_mp_enabled = Some(savemigration_mp_enabled);
    hooks.profanity_status = Some(profanity_status);
    hooks.profanity_pending = Some(profanity_pending);
    hooks.profanity_valid = Some(profanity_valid);
    hooks.social_club_account = Some(social_club_account);
    hooks.online_policies_current = Some(online_policies_current);
    hooks.match_history_id = Some(match_history_id);
    hooks.script_program_loaded = Some(script_program_loaded);
    hooks.presence_online = Some(presence_online);
    hooks.can_enter_multiplayer = Some(can_enter_multiplayer);
    hooks.populate_store_data = Some(populate_store_data);
    hooks.photo_to_jpeg = Some(photo_to_jpeg);
    hooks.can_access_network_features = Some(can_access_network_features);
}

#[test]
fn broadcast_data_keeps_the_length() {
    let item = |size: u32, n: u32| {
        format!(
            "\t\t<Item>\r\n\t\t\t<SizeOfData value=\"{size}\"/>\r\n\t\t\t<NumInstances value=\"{n}\"/>\r\n\t\t</Item>\r\n"
        )
    };
    let sizes = [(3600, 1), (2500, 2), (150, 4), (80, 4), (40, 8)];
    let text = format!(
        "<HostBroadcastData>\r\n{}</HostBroadcastData>\r\n<PlayerBroadcastData>\r\n{}</PlayerBroadcastData>\r\n",
        item(40, 8),
        sizes.map(|(s, n)| item(s, n)).concat()
    );
    let patched = more_broadcast_data(text.clone()).unwrap();
    assert_eq!(patched.len(), text.len());
    assert!(patched.starts_with(&format!("<HostBroadcastData>\r\n{}", item(40, 8))));
    assert!(patched.contains("<SizeOfData value=\"3600\"/>\r\n\t\t\t<NumInstances value=\"3\"/>"));
    assert!(patched.contains("<SizeOfData value=\"150\"/>\r\n\t\t<NumInstances value=\"12\"/>"));
    assert!(patched.contains("<SizeOfData value=\"40\"/>\r\n\t\t<NumInstances value=\"16\"/>"));
}
