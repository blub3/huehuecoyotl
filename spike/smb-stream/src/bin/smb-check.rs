//! Headless harness: drives the exact extern "C" callbacks mpv would call, and compares
//! every byte against a local copy of the same file.
//!
//!   smb-check coyotl://172.17.0.2/media/uhd-hevc10-sdr.mkv ../media/uhd-hevc10-sdr.mkv
use std::ffi::CString;
use std::time::Instant;

use huehuecoyotl_spike_stream::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (uri, local) = (&args[1], &args[2]);
    let reference = std::fs::read(local).expect("local reference file");

    let mut info = MpvStreamCbInfo {
        cookie: std::ptr::null_mut(),
        read_fn: None,
        seek_fn: None,
        size_fn: None,
        close_fn: None,
        cancel_fn: None,
    };
    let curi = CString::new(uri.as_str()).unwrap();
    let t0 = Instant::now();
    let rc = unsafe { coyotl_stream_open(std::ptr::null_mut(), curi.as_ptr() as *mut _, &mut info) };
    assert_eq!(rc, 0, "open failed");
    println!("open: {:?}", t0.elapsed());
    let (read, seek, size, close) = (
        info.read_fn.unwrap(),
        info.seek_fn.unwrap(),
        info.size_fn.unwrap(),
        info.close_fn.unwrap(),
    );
    let c = info.cookie;
    let n = unsafe { size(c) } as usize;
    assert_eq!(n, reference.len(), "size mismatch");

    // 1. Sequential read in mpv-sized pieces (mpv's stream layer asks for <= 1 MiB at a time).
    let mut buf = vec![0u8; 256 * 1024];
    let mut got = Vec::with_capacity(n);
    let t = Instant::now();
    loop {
        let r = unsafe { read(c, buf.as_mut_ptr().cast(), buf.len() as u64) };
        assert!(r >= 0, "read error");
        if r == 0 {
            break;
        }
        got.extend_from_slice(&buf[..r as usize]);
    }
    let secs = t.elapsed().as_secs_f64();
    assert!(got == reference, "sequential content mismatch");
    println!(
        "sequential: {} MiB in {:.2}s = {:.0} MiB/s ({:.0} Mbit/s), bytes identical",
        n >> 20,
        secs,
        n as f64 / secs / 1048576.0,
        n as f64 * 8.0 / secs / 1e6
    );

    // 2. Random seeks + short reads, like a player probing the index and seeking.
    let mut x: u64 = 0x9e3779b97f4a7c15;
    let t = Instant::now();
    let mut worst = 0f64;
    for _ in 0..200 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let off = (x % n as u64) as usize;
        let t1 = Instant::now();
        assert_eq!(unsafe { seek(c, off as i64) }, off as i64);
        let r = unsafe { read(c, buf.as_mut_ptr().cast(), 65536) } as usize;
        worst = worst.max(t1.elapsed().as_secs_f64());
        assert!(buf[..r] == reference[off..off + r], "content mismatch after seek to {off}");
    }
    println!(
        "200 random seek+read: avg {:.1} ms, worst {:.1} ms, bytes identical",
        t.elapsed().as_secs_f64() * 5.0,
        worst * 1000.0
    );
    println!("stats: {:?}", unsafe { stats(c) });
    unsafe { close(c) };
    println!("PASS");
}
