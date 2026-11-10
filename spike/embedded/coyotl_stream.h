#pragma once
// Rust-side (crates/stream in the product) entry point for mpv's stream callback API.
#include <mpv/stream_cb.h>

extern "C" int coyotl_stream_open(void *user_data, char *uri, mpv_stream_cb_info *info);
