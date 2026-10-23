//! The desktop entry point as a C function, for an executable linked from
//! this library and the game's (`platform/macos/link-app`): its `main` calls
//! `re5osx_main`.

#[unsafe(no_mangle)]
pub extern "C" fn re5osx_main() -> i32 {
    re5osx_runtime::exit::process(re5osx::main())
}
