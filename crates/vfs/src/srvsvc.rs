//! Issue #121: the share list of an SMB server through the server service (MS-SRVS
//! NetrShareEnum, level 1) over the `srvsvc` pipe, in NDR (version 2). The `smb` crate has a
//! share list too, but it speaks NDR64 only, which Samba (most NAS systems) refuses.

use smb::{IoctlBuffer, Pipe, PipeTransceiveRequest};

use crate::{Result, VfsError};

const SRVSVC: [u8; 16] = uuid(
    0x4b32_4fc8,
    0x1670,
    0x01d3,
    [0x12, 0x78, 0x5a, 0x47, 0xbf, 0x6e, 0xe1, 0x88],
);
const NDR: [u8; 16] = uuid(
    0x8a88_5d04,
    0x1ceb,
    0x11c9,
    [0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60],
);
const FRAG: u16 = 4280;
/// The most a reply may be; a server with more shares than fit is refused (the share is typed).
const MAX_REPLY: u32 = 65_535;

const fn uuid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ]
}

fn bad(what: &str) -> VfsError {
    VfsError::Unsupported(format!("share list: {what}"))
}

/// A connection-oriented DCE/RPC packet: the 16-byte header, then `body`.
fn packet(ptype: u8, call_id: u32, body: &[u8]) -> Vec<u8> {
    let mut p = vec![5, 0, ptype, 0x03, 0x10, 0, 0, 0];
    p.extend_from_slice(&((16 + body.len()) as u16).to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&call_id.to_le_bytes());
    p.extend_from_slice(body);
    p
}

fn bind() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&FRAG.to_le_bytes());
    b.extend_from_slice(&FRAG.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // association group
    b.extend_from_slice(&[1, 0, 0, 0]); // one context
    b.extend_from_slice(&0u16.to_le_bytes()); // context id
    b.extend_from_slice(&[1, 0]); // one transfer syntax
    b.extend_from_slice(&SRVSVC);
    b.extend_from_slice(&[3, 0, 0, 0]); // srvsvc 3.0
    b.extend_from_slice(&NDR);
    b.extend_from_slice(&2u32.to_le_bytes());
    packet(11, 1, &b)
}

fn put_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// NetrShareEnum(ServerName, level 1, an empty container, no limit, resume 0).
fn share_enum(server: &str) -> Vec<u8> {
    let name: Vec<u16> = format!("\\\\{server}").encode_utf16().chain([0]).collect();
    let mut s = Vec::new();
    put_u32(&mut s, 0x0002_0000); // ServerName: unique pointer
    put_u32(&mut s, name.len() as u32);
    put_u32(&mut s, 0);
    put_u32(&mut s, name.len() as u32);
    for c in &name {
        s.extend_from_slice(&c.to_le_bytes());
    }
    while s.len() % 4 != 0 {
        s.push(0);
    }
    put_u32(&mut s, 1); // level
    put_u32(&mut s, 1); // union switch
    put_u32(&mut s, 0x0002_0004); // the level-1 container
    put_u32(&mut s, 0); // entries read
    put_u32(&mut s, 0); // no buffer
    put_u32(&mut s, u32::MAX); // preferred maximum length
    put_u32(&mut s, 0x0002_0008); // resume handle pointer
    put_u32(&mut s, 0);
    let mut b = Vec::new();
    put_u32(&mut b, s.len() as u32); // allocation hint
    b.extend_from_slice(&0u16.to_le_bytes()); // context id
    b.extend_from_slice(&15u16.to_le_bytes()); // opnum: NetrShareEnum
    b.extend_from_slice(&s);
    packet(0, 2, &b)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32> {
        let v = self
            .b
            .get(self.at..self.at + 4)
            .ok_or_else(|| bad("short reply"))?;
        self.at += 4;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn string(&mut self) -> Result<String> {
        let _max = self.u32()?;
        let _offset = self.u32()?;
        let n = self.u32()? as usize;
        let raw = self
            .b
            .get(self.at..self.at + 2 * n)
            .ok_or_else(|| bad("short string"))?;
        self.at += 2 * n;
        self.at = self.at.div_ceil(4) * 4;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Ok(String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string())
    }
}

/// The disk shares (name, type) in a NetrShareEnum level-1 reply's stub data.
fn parse_shares(stub: &[u8]) -> Result<Vec<(String, u32)>> {
    let mut r = Reader { b: stub, at: 0 };
    if r.u32()? != 1 {
        return Err(bad("unexpected level"));
    }
    let _switch = r.u32()?;
    if r.u32()? == 0 {
        return Ok(vec![]);
    }
    let count = r.u32()? as usize;
    if r.u32()? == 0 {
        return Ok(vec![]);
    }
    let max = r.u32()? as usize;
    if max < count || count > 4096 {
        return Err(bad("bad count"));
    }
    let mut fixed = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.u32()?;
        let kind = r.u32()?;
        let remark = r.u32()?;
        fixed.push((name, kind, remark));
    }
    let mut out = Vec::with_capacity(count);
    for (name, kind, remark) in fixed {
        let n = if name != 0 {
            r.string()?
        } else {
            String::new()
        };
        if remark != 0 {
            r.string()?;
        }
        out.push((n, kind));
    }
    Ok(out)
}

/// The body of a reply packet (bind ack 12, response 2), or the server's refusal (fault 3).
fn reply(raw: &[u8], want: u8) -> Result<&[u8]> {
    if raw.len() < 16 || raw[0] != 5 {
        return Err(bad("not an RPC reply"));
    }
    if raw[2] == 3 {
        let status = raw
            .get(24..28)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        return Err(bad(&format!("refused ({:#x})", status.unwrap_or(0))));
    }
    if raw[2] != want {
        return Err(bad("unexpected reply"));
    }
    if raw[3] & 0x02 == 0 {
        return Err(bad("too many shares to list"));
    }
    let len = u16::from_le_bytes([raw[8], raw[9]]) as usize;
    raw.get(16..len.min(raw.len()))
        .ok_or_else(|| bad("short reply"))
}

async fn transceive(pipe: &Pipe, data: Vec<u8>) -> Result<Vec<u8>> {
    let res = pipe
        .fsctl_with_options(
            PipeTransceiveRequest::from(IoctlBuffer::from(data)),
            MAX_REPLY,
        )
        .await
        .map_err(|e| VfsError::Unsupported(format!("share list: {e}")))?;
    let bytes: &[u8] = res.as_ref();
    Ok(bytes.to_vec())
}

/// The shares of `server` through an open `srvsvc` pipe: (name, type), type as in MS-SRVS
/// (0 a disk, 0x8000_0000 set for special shares such as `IPC$`).
pub(crate) async fn list(pipe: &Pipe, server: &str) -> Result<Vec<(String, u32)>> {
    let ack = transceive(pipe, bind()).await?;
    let body = reply(&ack, 12)?;
    // max_xmit, max_recv, group, then the secondary address (length, bytes), padded to 4.
    let addr =
        u16::from_le_bytes([*body.get(8).ok_or_else(|| bad("short ack"))?, body[9]]) as usize;
    let at = (10 + addr).div_ceil(4) * 4;
    let result = body.get(at + 4..at + 6).ok_or_else(|| bad("short ack"))?;
    if result != [0, 0] {
        return Err(bad("the server refused the share list"));
    }
    let res = transceive(pipe, share_enum(server)).await?;
    let body = reply(&res, 2)?;
    // alloc hint, context id, cancel count, reserved; then the stub.
    parse_shares(body.get(8..).ok_or_else(|| bad("short response"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_level_one_reply() {
        let mut s = Vec::new();
        for x in [1u32, 1, 0x20000, 2, 0x20004, 2] {
            put_u32(&mut s, x);
        }
        for x in [0x20008u32, 0, 0x2000c, 0x20010, 0x8000_0003, 0] {
            put_u32(&mut s, x);
        }
        let mut string = |t: &str| {
            let u: Vec<u16> = t.encode_utf16().chain([0]).collect();
            for x in [u.len() as u32, 0, u.len() as u32] {
                put_u32(&mut s, x);
            }
            for c in u {
                s.extend_from_slice(&c.to_le_bytes());
            }
            while s.len() % 4 != 0 {
                s.push(0);
            }
        };
        string("media");
        string("Movies and more");
        string("IPC$");
        assert_eq!(
            parse_shares(&s).unwrap(),
            [("media".to_string(), 0), ("IPC$".to_string(), 0x8000_0003)]
        );
    }
}
