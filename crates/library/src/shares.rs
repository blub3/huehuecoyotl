//! Network shares (2.8, issue #118, SPEC 6 "Shares"): a share is one server, one share (or
//! export path) and one user, with one set of connection settings and one keystore entry; it
//! holds any number of library folders (`source` rows with `share_id`). Each folder keeps its own
//! root, content type, files and items, so scans, playback and saved copies work as before.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::Result;

/// What makes two folders the same share: the protocol, the server (any case), the port, the
/// share or export path, the user and the key file. Not the host key or other options.
pub fn group_key(kind: &str, config_json: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(config_json).unwrap_or_default();
    let s = |k: &str| v[k].as_str().unwrap_or_default().trim().to_string();
    format!(
        "{kind}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
        s("host").to_lowercase(),
        v["port"]
            .as_u64()
            .map(|p| p.to_string())
            .unwrap_or_default(),
        // SMB share names do not depend on case; NFS, SFTP and WebDAV paths do.
        {
            let share = s("share").trim_matches(['/', '\\']).to_string();
            if kind == "smb" {
                share.to_lowercase()
            } else {
                share
            }
        },
        s("user"),
        s("key_file"),
    )
}

/// A display name: "media on nas.local", or the server alone.
pub fn default_name(config_json: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(config_json).unwrap_or_default();
    let host = v["host"].as_str().unwrap_or_default();
    let share = v["share"].as_str().unwrap_or_default().trim_matches('/');
    if share.is_empty() {
        host.to_string()
    } else {
        format!("{share} on {host}")
    }
}

/// The keystore reference of a share's password.
pub fn credential_ref(id: i64) -> String {
    format!("share:{id}")
}

/// The share these settings belong to, if one exists.
pub fn find(c: &Connection, kind: &str, config_json: &str) -> Result<Option<i64>> {
    let key = group_key(kind, config_json);
    let mut st = c.prepare("SELECT id, kind, config_json FROM share ORDER BY id")?;
    let rows: Vec<(i64, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows
        .into_iter()
        .find(|(_, k, cfg)| group_key(k, cfg) == key)
        .map(|(id, _, _)| id))
}

/// The share for these settings, created when there is none. Returns (id, created).
pub fn find_or_create(c: &Connection, kind: &str, config_json: &str) -> Result<(i64, bool)> {
    if let Some(id) = find(c, kind, config_json)? {
        return Ok((id, false));
    }
    c.execute(
        "INSERT INTO share(kind, name, config_json) VALUES (?1, ?2, ?3)",
        params![kind, default_name(config_json), config_json],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "UPDATE share SET credential_ref = ?2 WHERE id = ?1",
        params![id, credential_ref(id)],
    )?;
    Ok((id, true))
}

/// Store a share's updated settings (an SFTP host key seen for the first time).
pub fn set_config(c: &Connection, id: i64, config_json: &str) -> Result<()> {
    c.execute(
        "UPDATE share SET config_json = ?2 WHERE id = ?1",
        params![id, config_json],
    )?;
    Ok(())
}

/// One folder of a share.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub source_id: i64,
    pub name: String,
    /// The folder inside the share ("" = the whole share).
    pub root: String,
    pub content: String,
    pub files: i64,
    pub last_scan: i64,
}

/// A share with its folders.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub id: i64,
    pub kind: String,
    pub name: String,
    /// The settings without the password.
    #[serde(skip)]
    pub config_json: String,
    pub folders: Vec<Folder>,
}

pub fn list(c: &Connection) -> Result<Vec<Share>> {
    let mut st = c.prepare("SELECT id, kind, name, config_json FROM share ORDER BY name, id")?;
    let mut shares: Vec<Share> = st
        .query_map([], |r| {
            Ok(Share {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                config_json: r.get(3)?,
                folders: vec![],
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut st = c.prepare(
        "SELECT s.share_id, s.id, s.name, coalesce(s.root, ''), s.content,
                (SELECT count(*) FROM file f WHERE f.source_id = s.id), coalesce(s.last_scan, 0)
         FROM source s WHERE s.share_id IS NOT NULL ORDER BY s.root, s.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Folder {
                source_id: r.get(1)?,
                name: r.get(2)?,
                root: r.get(3)?,
                content: r.get(4)?,
                files: r.get(5)?,
                last_scan: r.get(6)?,
            },
        ))
    })?;
    for row in rows {
        let (share, folder) = row?;
        if let Some(s) = shares.iter_mut().find(|s| s.id == share) {
            s.folders.push(folder);
        }
    }
    Ok(shares)
}

/// Issue #122: a share's new connection settings, for the share and its folders. A name that
/// was the default one ("media on nas") follows the new settings.
pub fn update(c: &Connection, id: i64, kind: &str, config_json: &str) -> Result<()> {
    let (name, old): (String, String) = c.query_row(
        "SELECT name, config_json FROM share WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let name = if name == default_name(&old) {
        default_name(config_json)
    } else {
        name
    };
    c.execute(
        "UPDATE share SET kind = ?2, name = ?3, config_json = ?4 WHERE id = ?1",
        params![id, kind, name, config_json],
    )?;
    c.execute(
        "UPDATE source SET kind = ?2, config_json = ?3 WHERE share_id = ?1",
        params![id, kind, config_json],
    )?;
    Ok(())
}

/// Issue #122: the folders of share `from` that share `into` has already (same folder, same
/// content type); they go before the shares are joined.
pub fn duplicates(c: &Connection, into: i64, from: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT f.id FROM source f JOIN source i ON i.share_id = ?1 AND i.root = f.root AND i.content = f.content
         WHERE f.share_id = ?2 ORDER BY f.id",
    )?;
    Ok(st
        .query_map(params![into, from], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Issue #122: join share `from` into share `into` (the same server, reached under two names):
/// its folders move with their files, items and play state, and `from` goes. Returns the folders
/// moved and `from`'s keystore reference.
pub fn join(c: &Connection, into: i64, from: i64) -> Result<(Vec<i64>, Option<String>)> {
    let moved = folder_ids(c, from)?;
    let (kind, config_json): (String, String) = c.query_row(
        "SELECT kind, config_json FROM share WHERE id = ?1",
        [into],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    c.execute(
        "UPDATE source SET share_id = ?1, credential_ref = ?2, kind = ?3, config_json = ?4 WHERE share_id = ?5",
        params![into, credential_ref(into), kind, config_json, from],
    )?;
    Ok((moved, remove(c, from)?))
}

pub fn get(c: &Connection, id: i64) -> Result<Option<Share>> {
    Ok(list(c)?.into_iter().find(|s| s.id == id))
}

/// Add a folder to a share (the same folder with the same content type is not added twice).
/// Returns (source id, created).
pub fn add_folder(
    c: &Connection,
    share: i64,
    root: &str,
    content: &str,
    name: &str,
) -> Result<(i64, bool)> {
    let root = root.trim_end_matches('/');
    let (kind, config_json): (String, String) = c.query_row(
        "SELECT kind, config_json FROM share WHERE id = ?1",
        [share],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if let Some(id) = c
        .query_row(
            "SELECT id FROM source WHERE share_id = ?1 AND root = ?2 AND content = ?3",
            params![share, root, content],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok((id, false));
    }
    c.execute(
        "INSERT INTO source(kind, name, config_json, root, content, share_id, credential_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            kind,
            name,
            config_json,
            root,
            content,
            share,
            credential_ref(share)
        ],
    )?;
    Ok((c.last_insert_rowid(), true))
}

/// The folders of a share, to remove before the share itself.
pub fn folder_ids(c: &Connection, share: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare("SELECT id FROM source WHERE share_id = ?1 ORDER BY id")?;
    Ok(st
        .query_map([share], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Remove a share whose folders are gone. Returns its keystore reference.
pub fn remove(c: &Connection, share: i64) -> Result<Option<String>> {
    let reference: Option<String> = c
        .query_row(
            "SELECT credential_ref FROM share WHERE id = ?1",
            [share],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    c.execute(
        "UPDATE source SET share_id = NULL WHERE share_id = ?1",
        [share],
    )?;
    c.execute("DELETE FROM share WHERE id = ?1", [share])?;
    Ok(reference)
}

/// The keystore entries of a share's folders from before shares (`source:<id>`), newest first:
/// where its password may still be.
pub fn legacy_credentials(c: &Connection, share: i64) -> Result<Vec<String>> {
    let mut st =
        c.prepare("SELECT 'source:' || id FROM source WHERE share_id = ?1 ORDER BY id DESC")?;
    Ok(st
        .query_map([share], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Migration 13 (issue #118): network folders with the same server, share and user become one
/// share. The first folder's settings (one with a host key preferred) become the share's.
pub fn migration_13(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE share(id INTEGER PRIMARY KEY,
                            kind TEXT NOT NULL,
                            name TEXT NOT NULL,
                            config_json TEXT NOT NULL,
                            credential_ref TEXT);
         ALTER TABLE source ADD COLUMN share_id INTEGER REFERENCES share(id);
         CREATE INDEX source_share ON source(share_id);",
    )?;
    let rows: Vec<(i64, String, String)> = {
        let mut st = tx.prepare(
            "SELECT id, kind, config_json FROM source WHERE kind <> 'local' AND root IS NOT NULL ORDER BY id",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    // (group key, [(source, kind, settings)])
    type Group = (String, Vec<(i64, String, String)>);
    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        let key = group_key(&row.1, &row.2);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(row),
            None => groups.push((key, vec![row])),
        }
    }
    for (_, g) in groups {
        let with_key = g
            .iter()
            .find(|(_, _, cfg)| {
                serde_json::from_str::<serde_json::Value>(cfg)
                    .is_ok_and(|v| v["host_key"].is_string())
            })
            .unwrap_or(&g[0]);
        let (kind, cfg) = (with_key.1.clone(), with_key.2.clone());
        tx.execute(
            "INSERT INTO share(kind, name, config_json) VALUES (?1, ?2, ?3)",
            params![kind, default_name(&cfg), cfg],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE share SET credential_ref = ?2 WHERE id = ?1",
            params![id, credential_ref(id)],
        )?;
        for (source, _, _) in &g {
            tx.execute(
                "UPDATE source SET share_id = ?2 WHERE id = ?1",
                params![source, id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn keys_ignore_case_and_host_keys() {
        let a = r#"{"protocol":"smb","host":"NAS.local","share":"/media/","user":"me"}"#;
        let b =
            r#"{"protocol":"smb","host":"nas.local","share":"media","user":"me","host_key":"x"}"#;
        let upper = r#"{"protocol":"smb","host":"nas.local","share":"MEDIA","user":"me"}"#;
        assert_eq!(group_key("smb", a), group_key("smb", upper));
        let nfs_upper = r#"{"protocol":"nfs","host":"nas","share":"/Volume1"}"#;
        assert_ne!(
            group_key("nfs", nfs_upper),
            group_key("nfs", &nfs_upper.to_lowercase())
        );
        let other_user = r#"{"protocol":"smb","host":"nas.local","share":"media","user":"you"}"#;
        assert_eq!(group_key("smb", a), group_key("smb", b));
        assert_ne!(group_key("smb", a), group_key("smb", other_user));
        assert_ne!(group_key("smb", a), group_key("nfs", a));
        assert_eq!(default_name(a), "media on NAS.local");
    }

    #[test]
    fn folders_of_one_share() {
        let c = db();
        let cfg = r#"{"protocol":"smb","host":"nas","share":"media","user":"me"}"#;
        let (s, created) = find_or_create(&c, "smb", cfg).unwrap();
        assert!(created);
        assert_eq!(
            find_or_create(&c, "smb", &cfg.replace("nas", "NAS")).unwrap(),
            (s, false)
        );
        let (m, _) = add_folder(&c, s, "/Movies/", "video", "Movies").unwrap();
        let (music, _) = add_folder(&c, s, "/Music", "music", "Music").unwrap();
        assert_eq!(
            add_folder(&c, s, "/Movies", "video", "Movies").unwrap(),
            (m, false)
        );
        let list = list(&c).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0]
                .folders
                .iter()
                .map(|f| f.source_id)
                .collect::<Vec<_>>(),
            [m, music]
        );
        assert_eq!(
            legacy_credentials(&c, s).unwrap(),
            [format!("source:{music}"), format!("source:{m}")]
        );
        assert_eq!(folder_ids(&c, s).unwrap(), [m, music]);
        assert_eq!(
            remove(&c, s).unwrap().as_deref(),
            Some(credential_ref(s).as_str())
        );
        assert!(super::list(&c).unwrap().is_empty());
    }

    #[test]
    fn update_and_join() {
        let c = db();
        let by_name = r#"{"protocol":"smb","host":"nas","share":"media","user":"me"}"#;
        let by_ip = r#"{"protocol":"smb","host":"192.168.1.20","share":"media","user":"me"}"#;
        let (a, _) = find_or_create(&c, "smb", by_name).unwrap();
        let (b, _) = find_or_create(&c, "smb", by_ip).unwrap();
        let (movies, _) = add_folder(&c, a, "/Movies", "video", "Movies").unwrap();
        let (dup, _) = add_folder(&c, b, "/Movies", "video", "Movies").unwrap();
        let (music, _) = add_folder(&c, b, "/Music", "music", "Music").unwrap();
        c.execute("UPDATE share SET name = 'My NAS' WHERE id = ?1", [b])
            .unwrap();

        // A new address: the share and its folders follow; a default name too.
        update(&c, a, "smb", by_ip).unwrap();
        let s = get(&c, a).unwrap().unwrap();
        assert_eq!(s.name, "media on 192.168.1.20");
        let folder_cfg: String = c
            .query_row(
                "SELECT config_json FROM source WHERE id = ?1",
                [movies],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(folder_cfg, by_ip);

        // Now both are the same share: b's copy of Movies goes, Music moves to a.
        assert_eq!(duplicates(&c, a, b).unwrap(), [dup]);
        c.execute("DELETE FROM source WHERE id = ?1", [dup])
            .unwrap();
        let (moved, reference) = join(&c, a, b).unwrap();
        assert_eq!(moved, [music]);
        assert_eq!(reference.as_deref(), Some(credential_ref(b).as_str()));
        let all = list(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0]
                .folders
                .iter()
                .map(|f| f.source_id)
                .collect::<Vec<_>>(),
            [movies, music]
        );
        let r: String = c
            .query_row(
                "SELECT credential_ref FROM source WHERE id = ?1",
                [music],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(r, credential_ref(a));
    }
}
