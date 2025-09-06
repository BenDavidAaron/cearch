use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::Once;

// Register sqlite-vec extension globally once so new connections auto-load it.
fn ensure_vec_extension_loaded() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        use rusqlite::ffi::sqlite3_auto_extension;
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

fn f32s_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

pub struct DB {
    conn: Connection,
}

impl DB {
    pub fn open_with_dim(repo_root: &Path, dim: usize) -> Result<Self> {
        let db_path = repo_root.join(".cearch").join("index.sqlite");
        std::fs::create_dir_all(db_path.parent().unwrap())?;
        ensure_vec_extension_loaded();
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL,
                line INTEGER NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                code TEXT NOT NULL
            );
            "#,
        )?;
        // Create vector index table with specified dimension if not exists
        let sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_index USING vec0(embedding float[{}]);",
            dim
        );
        conn.execute_batch(&sql)?;
        Ok(DB { conn })
    }

    pub fn open_read(repo_root: &Path) -> Result<Self> {
        let db_path = repo_root.join(".cearch").join("index.sqlite");
        ensure_vec_extension_loaded();
        let conn = Connection::open(db_path)?;
        Ok(DB { conn })
    }

    pub fn insert_symbol(
        &self,
        path: &Path,
        line: usize,
        kind: &str,
        name: &str,
        code: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "INSERT INTO symbols(path,line,kind,name,code) VALUES(?,?,?,?,?)",
            params![path.to_string_lossy(), line as i64, kind, name, code],
        )?;
        // rowid of last insert
        let rowid = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO vec_index(rowid, embedding) VALUES(?1, ?2)",
            rusqlite::params![rowid, f32s_to_blob(embedding)],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn knn(&self, query: &[f32], k: usize) -> Result<Vec<(PathBuf, usize, String, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.path, s.line, s.name, v.distance \
             FROM ( \
               SELECT rowid, distance \
               FROM vec_index \
               WHERE embedding MATCH ?1 \
               ORDER BY distance \
               LIMIT ?2 \
             ) AS v \
             JOIN symbols s ON s.id = v.rowid \
             ORDER BY v.distance",
        )?;
        let rows = stmt.query_map(params![f32s_to_blob(query), k as i64], |row| {
            let path: String = row.get(0)?;
            let line: i64 = row.get(1)?;
            let name: String = row.get(2)?;
            let dist: f32 = row.get(3)?;
            Ok((PathBuf::from(path), line as usize, name, dist))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}
