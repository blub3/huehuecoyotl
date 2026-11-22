//! Picks the FFmpeg types the probe is built with. Development and bundle builds use FFmpeg 8.1
//! (`ffi_types.rs`). The Flatpak uses the runtime's FFmpeg 7.1 (issue #161) and sets
//! `COYOTL_FFMPEG_ABI=7.1`, which selects `ffi_types_71.rs`.

fn main() {
    println!("cargo:rerun-if-env-changed=COYOTL_FFMPEG_ABI");
    println!("cargo:rustc-check-cfg=cfg(ffmpeg71)");
    match std::env::var("COYOTL_FFMPEG_ABI").as_deref() {
        Ok("7.1") => println!("cargo:rustc-cfg=ffmpeg71"),
        Ok("") | Ok("8.1") | Err(_) => {}
        Ok(other) => panic!("COYOTL_FFMPEG_ABI={other}: only 7.1 and 8.1 are supported"),
    }
}
