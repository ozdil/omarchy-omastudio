//! SQLite (WAL) catalog backend via the system `libsqlite3` shared library.
//!
//! Stores indexed photo metadata and culling summary fields in
//! `$XDG_DATA_HOME/omastudio/catalog.db` while keeping full edit recipes in
//! per-photo `<photo>.omastudio` sidecars.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::io;
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::ptr;

use crate::raw::RawMetadata;
use crate::recipe::{CatalogFilter, CatalogItem, Recipe};

#[repr(C)]
struct Sqlite3 {
    _private: [u8; 0],
}

#[repr(C)]
struct Sqlite3Stmt {
    _private: [u8; 0],
}

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_NULL: c_int = 5;

const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;
const SQLITE_OPEN_NOFOLLOW: c_int = 0x0100_0000;

/// Equivalent to C `SQLITE_TRANSIENT` (`(void(*)(void*))-1`), instructing SQLite to copy bound text.
const SQLITE_TRANSIENT: isize = -1;

// SAFETY: Symbols belong to the system `libsqlite3` linked via pkg-config in build.rs;
// C ABI signatures match `sqlite3.h` (>= 3.35).
unsafe extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut Sqlite3,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut Sqlite3) -> c_int;
    fn sqlite3_exec(
        db: *mut Sqlite3,
        sql: *const c_char,
        callback: *const c_void,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(ptr: *mut c_void);
    fn sqlite3_errmsg(db: *mut Sqlite3) -> *const c_char;
    fn sqlite3_changes(db: *mut Sqlite3) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut Sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut Sqlite3Stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_finalize(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        val: *const c_char,
        n_bytes: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut Sqlite3Stmt, index: c_int, val: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut Sqlite3Stmt, index: c_int, val: c_double) -> c_int;
    fn sqlite3_bind_null(stmt: *mut Sqlite3Stmt, index: c_int) -> c_int;
    fn sqlite3_column_type(stmt: *mut Sqlite3Stmt, col: c_int) -> c_int;
    fn sqlite3_column_text(stmt: *mut Sqlite3Stmt, col: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut Sqlite3Stmt, col: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut Sqlite3Stmt, col: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut Sqlite3Stmt, col: c_int) -> c_double;
}

const SCHEMA_SQL: &str = "\
PRAGMA journal_mode=WAL;\
PRAGMA synchronous=NORMAL;\
PRAGMA busy_timeout=5000;\
CREATE TABLE IF NOT EXISTS photos (\
    id INTEGER PRIMARY KEY,\
    path TEXT NOT NULL UNIQUE,\
    mtime_ns INTEGER NOT NULL DEFAULT 0,\
    rating INTEGER NOT NULL DEFAULT 0,\
    flag TEXT NOT NULL DEFAULT 'none',\
    color_label TEXT NOT NULL DEFAULT '',\
    item_id TEXT NOT NULL DEFAULT '',\
    remote_path TEXT,\
    thumbnail_path TEXT NOT NULL DEFAULT '',\
    camera_make TEXT NOT NULL DEFAULT '',\
    camera_model TEXT NOT NULL DEFAULT '',\
    lens_model TEXT NOT NULL DEFAULT '',\
    film_sim TEXT NOT NULL DEFAULT '',\
    iso REAL NOT NULL DEFAULT 0,\
    shutter REAL NOT NULL DEFAULT 0,\
    aperture REAL NOT NULL DEFAULT 0,\
    focal_mm REAL NOT NULL DEFAULT 0,\
    width INTEGER NOT NULL DEFAULT 0,\
    height INTEGER NOT NULL DEFAULT 0,\
    raw_width INTEGER NOT NULL DEFAULT 0,\
    raw_height INTEGER NOT NULL DEFAULT 0,\
    timestamp INTEGER NOT NULL DEFAULT 0,\
    cam_mul_json TEXT NOT NULL DEFAULT '[1.0,1.0,1.0,1.0]',\
    sidecar_hash TEXT NOT NULL DEFAULT '',\
    updated_at INTEGER NOT NULL DEFAULT 0\
);\
CREATE INDEX IF NOT EXISTS idx_photos_rating_flag ON photos(rating, flag);\
CREATE INDEX IF NOT EXISTS idx_photos_film_sim ON photos(film_sim);";

const UPSERT_SQL: &str = "\
INSERT INTO photos (\
    path, mtime_ns, rating, flag, color_label, item_id, remote_path, thumbnail_path,\
    camera_make, camera_model, lens_model, film_sim, iso, shutter, aperture, focal_mm,\
    width, height, raw_width, raw_height, timestamp, cam_mul_json, sidecar_hash, updated_at\
) VALUES (\
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,\
    ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,\
    ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24\
) ON CONFLICT(path) DO UPDATE SET \
    mtime_ns=excluded.mtime_ns,\
    rating=excluded.rating,\
    flag=excluded.flag,\
    color_label=excluded.color_label,\
    item_id=excluded.item_id,\
    remote_path=excluded.remote_path,\
    thumbnail_path=excluded.thumbnail_path,\
    camera_make=excluded.camera_make,\
    camera_model=excluded.camera_model,\
    lens_model=excluded.lens_model,\
    film_sim=excluded.film_sim,\
    iso=excluded.iso,\
    shutter=excluded.shutter,\
    aperture=excluded.aperture,\
    focal_mm=excluded.focal_mm,\
    width=excluded.width,\
    height=excluded.height,\
    raw_width=excluded.raw_width,\
    raw_height=excluded.raw_height,\
    timestamp=excluded.timestamp,\
    cam_mul_json=excluded.cam_mul_json,\
    sidecar_hash=excluded.sidecar_hash,\
    updated_at=excluded.updated_at";

const SELECT_COLUMNS: &str = "\
SELECT path, rating, flag, color_label, item_id, remote_path, thumbnail_path,\
       camera_make, camera_model, lens_model, film_sim, iso, shutter, aperture, focal_mm,\
       width, height, raw_width, raw_height, timestamp, cam_mul_json, updated_at \
FROM photos";

pub struct SqliteDb {
    raw: *mut Sqlite3,
}

impl Drop for SqliteDb {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `self.raw` is a valid SQLite handle owned by `SqliteDb` and closed once on drop.
            unsafe {
                sqlite3_close_v2(self.raw);
            }
            self.raw = ptr::null_mut();
        }
    }
}

struct SqliteStmt<'db> {
    raw: *mut Sqlite3Stmt,
    db: *mut Sqlite3,
    _marker: PhantomData<&'db SqliteDb>,
}

impl Drop for SqliteStmt<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: `self.raw` is a valid prepared statement tied to a live `SqliteDb` via `'db`.
            unsafe {
                sqlite3_finalize(self.raw);
            }
            self.raw = ptr::null_mut();
        }
    }
}

fn db_error(db: *mut Sqlite3, context: &str) -> io::Error {
    let detail = if db.is_null() {
        "null sqlite handle".to_string()
    } else {
        // SAFETY: `db` is non-null and `sqlite3_errmsg` returns a valid NUL-terminated UTF-8 string.
        let p = unsafe { sqlite3_errmsg(db) };
        if p.is_null() {
            "unknown sqlite error".to_string()
        } else {
            // SAFETY: `p` is guaranteed non-null and NUL-terminated by SQLite.
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    };
    io::Error::new(io::ErrorKind::InvalidData, format!("{context}: {detail}"))
}

impl SqliteDb {
    /// Opens or creates the SQLite catalog database in WAL mode with `0600` permissions.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            crate::security::ensure_secure_dir(parent)?;
        }
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && meta.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Refusing to open symlinked catalog database",
            ));
        }

        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Catalog path contains NUL byte"))?;
        let mut raw: *mut Sqlite3 = ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_NOFOLLOW;
        // SAFETY: `c_path` is a NUL-terminated path string and `raw` is a valid out-pointer.
        let rc = unsafe { sqlite3_open_v2(c_path.as_ptr(), &mut raw, flags, ptr::null()) };
        if rc != SQLITE_OK {
            let err = db_error(raw, "sqlite3_open_v2");
            if !raw.is_null() {
                // SAFETY: SQLite allocates a handle even on failed open so callers can inspect `errmsg`.
                unsafe {
                    sqlite3_close_v2(raw);
                }
            }
            return Err(err);
        }

        let db = Self { raw };
        // Enforce private 0600 mode on newly created database files.
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        db.exec(SCHEMA_SQL)?;
        Ok(db)
    }

    pub fn exec(&self, sql: &str) -> io::Result<()> {
        let c_sql = CString::new(sql)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "SQL contains NUL byte"))?;
        let mut errmsg: *mut c_char = ptr::null_mut();
        // SAFETY: `self.raw` is a live SQLite handle and `c_sql` is NUL-terminated.
        let rc = unsafe {
            sqlite3_exec(self.raw, c_sql.as_ptr(), ptr::null(), ptr::null_mut(), &mut errmsg)
        };
        if rc != SQLITE_OK {
            let msg = if !errmsg.is_null() {
                // SAFETY: SQLite populates `errmsg` with a heap-allocated NUL-terminated string.
                let s = unsafe { CStr::from_ptr(errmsg) }.to_string_lossy().into_owned();
                // SAFETY: `errmsg` was allocated by `sqlite3_exec` and must be freed via `sqlite3_free`.
                unsafe { sqlite3_free(errmsg.cast()) };
                io::Error::new(io::ErrorKind::InvalidData, format!("sqlite3_exec: {s}"))
            } else {
                db_error(self.raw, "sqlite3_exec")
            };
            return Err(msg);
        }
        Ok(())
    }

    fn prepare(&self, sql: &str) -> io::Result<SqliteStmt<'_>> {
        let c_sql = CString::new(sql)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "SQL contains NUL byte"))?;
        let mut stmt: *mut Sqlite3Stmt = ptr::null_mut();
        // SAFETY: `self.raw` is live, `c_sql` is NUL-terminated, and `stmt` is a valid out-pointer.
        let rc = unsafe {
            sqlite3_prepare_v2(self.raw, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut())
        };
        if rc != SQLITE_OK || stmt.is_null() {
            return Err(db_error(self.raw, "sqlite3_prepare_v2"));
        }
        Ok(SqliteStmt {
            raw: stmt,
            db: self.raw,
            _marker: PhantomData,
        })
    }

    fn changes(&self) -> usize {
        // SAFETY: `self.raw` is a live SQLite handle.
        unsafe { sqlite3_changes(self.raw) }.max(0) as usize
    }

    /// Reads all catalog entries from the `photos` table.
    pub fn load_all(&self) -> io::Result<HashMap<String, CatalogItem>> {
        let mut stmt = self.prepare(SELECT_COLUMNS)?;
        let mut items = HashMap::new();
        while stmt.step_row()? {
            let item = read_catalog_item(&stmt);
            items.insert(item.path.clone(), item);
        }
        Ok(items)
    }

    /// Replaces the full catalog contents inside a single transaction.
    pub fn replace_all(&self, items: &HashMap<String, CatalogItem>) -> io::Result<()> {
        self.exec("BEGIN IMMEDIATE")?;
        let res = (|| -> io::Result<()> {
            self.exec("DELETE FROM photos")?;
            let mut stmt = self.prepare(UPSERT_SQL)?;
            for item in items.values() {
                bind_catalog_item(&mut stmt, item)?;
                stmt.step_done()?;
                stmt.reset();
            }
            Ok(())
        })();
        match res {
            Ok(()) => self.exec("COMMIT"),
            Err(e) => {
                let _ = self.exec("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Atomically upserts a single catalog entry without rewriting the entire database.
    pub fn upsert_item(&self, item: &CatalogItem) -> io::Result<()> {
        let mut stmt = self.prepare(UPSERT_SQL)?;
        bind_catalog_item(&mut stmt, item)?;
        stmt.step_done()
    }

    /// Updates culling fields for one photo directly in SQLite. Returns `true` if the row existed.
    pub fn update_culling(
        &self,
        path: &str,
        rating: Option<u8>,
        flag: Option<&str>,
        color_label: Option<&str>,
        updated_at: u64,
    ) -> io::Result<bool> {
        let mut sets = vec!["updated_at = ?1".to_string()];
        enum BindVal {
            Int(i64),
            Text(String),
        }
        let mut vals = vec![BindVal::Int(updated_at as i64)];
        if let Some(r) = rating {
            vals.push(BindVal::Int(i64::from(r.min(5))));
            sets.push(format!("rating = ?{}", vals.len()));
        }
        if let Some(f) = flag {
            vals.push(BindVal::Text(f.to_string()));
            sets.push(format!("flag = ?{}", vals.len()));
        }
        if let Some(cl) = color_label {
            vals.push(BindVal::Text(cl.to_string()));
            sets.push(format!("color_label = ?{}", vals.len()));
        }
        vals.push(BindVal::Text(path.to_string()));
        let sql = format!(
            "UPDATE photos SET {} WHERE path = ?{}",
            sets.join(", "),
            vals.len()
        );
        let mut stmt = self.prepare(&sql)?;
        for (idx, val) in vals.iter().enumerate() {
            let p = (idx + 1) as c_int;
            match val {
                BindVal::Int(v) => stmt.bind_i64(p, *v)?,
                BindVal::Text(s) => stmt.bind_text(p, s)?,
            }
        }
        stmt.step_done()?;
        Ok(self.changes() > 0)
    }

    /// Executes an indexed SQL query matching `filter`, ordered by `updated_at DESC`.
    pub fn query(&self, filter: &CatalogFilter) -> io::Result<Vec<CatalogItem>> {
        enum BindVal {
            Int(i64),
            Float(f64),
            Text(String),
        }
        let mut clauses: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();

        if let Some(min_r) = filter.min_rating {
            binds.push(BindVal::Int(i64::from(min_r)));
            clauses.push(format!("rating >= ?{}", binds.len()));
        }
        if let Some(ref fl) = filter.flag
            && !fl.is_empty()
        {
            if fl == "none" || fl == "unflagged" {
                clauses.push("(flag = '' OR flag = 'none' OR flag = 'unflagged')".to_string());
            } else {
                binds.push(BindVal::Text(fl.clone()));
                clauses.push(format!("flag = ?{}", binds.len()));
            }
        }
        if let Some(ref make) = filter.camera_make
            && !make.is_empty()
        {
            binds.push(BindVal::Text(like_pattern(make)));
            clauses.push(format!("LOWER(camera_make) LIKE ?{} ESCAPE '\\'", binds.len()));
        }
        if let Some(ref model) = filter.camera_model
            && !model.is_empty()
        {
            binds.push(BindVal::Text(like_pattern(model)));
            clauses.push(format!("LOWER(camera_model) LIKE ?{} ESCAPE '\\'", binds.len()));
        }
        if let Some(ref lens) = filter.lens
            && !lens.is_empty()
        {
            binds.push(BindVal::Text(like_pattern(lens)));
            clauses.push(format!("LOWER(lens_model) LIKE ?{} ESCAPE '\\'", binds.len()));
        }
        if let Some(min_iso) = filter.min_iso {
            binds.push(BindVal::Float(f64::from(min_iso)));
            clauses.push(format!("iso >= ?{}", binds.len()));
        }
        if let Some(max_iso) = filter.max_iso {
            binds.push(BindVal::Float(f64::from(max_iso)));
            clauses.push(format!("iso <= ?{}", binds.len()));
        }
        if let Some(ref q) = filter.search_query
            && !q.is_empty()
        {
            binds.push(BindVal::Text(like_pattern(q)));
            let p = binds.len();
            clauses.push(format!(
                "(LOWER(path) LIKE ?{p} ESCAPE '\\' OR LOWER(camera_model) LIKE ?{p} ESCAPE '\\' OR LOWER(lens_model) LIKE ?{p} ESCAPE '\\')"
            ));
        }

        let mut sql = String::from(SELECT_COLUMNS);
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC");

        let mut stmt = self.prepare(&sql)?;
        for (idx, val) in binds.iter().enumerate() {
            let p = (idx + 1) as c_int;
            match val {
                BindVal::Int(v) => stmt.bind_i64(p, *v)?,
                BindVal::Float(v) => stmt.bind_f64(p, *v)?,
                BindVal::Text(s) => stmt.bind_text(p, s)?,
            }
        }

        let mut out = Vec::new();
        while stmt.step_row()? {
            out.push(read_catalog_item(&stmt));
        }
        Ok(out)
    }

    /// Reads the active SQLite journal mode (used in unit tests to verify WAL).
    #[cfg(test)]
    pub fn journal_mode(&self) -> io::Result<String> {
        let mut stmt = self.prepare("PRAGMA journal_mode")?;
        if stmt.step_row()? {
            Ok(stmt.col_text(0))
        } else {
            Ok(String::new())
        }
    }
}

impl SqliteStmt<'_> {
    fn bind_text(&mut self, index: c_int, s: &str) -> io::Result<()> {
        let len = c_int::try_from(s.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "SQLite text value too long"))?;
        let ptr = if s.is_empty() {
            c"".as_ptr()
        } else {
            s.as_ptr().cast::<c_char>()
        };
        // SAFETY: `self.raw` is a live prepared statement, `ptr` points to `len` valid UTF-8 bytes,
        // and `SQLITE_TRANSIENT` instructs SQLite to copy the bytes before returning.
        let rc = unsafe { sqlite3_bind_text(self.raw, index, ptr, len, SQLITE_TRANSIENT) };
        if rc != SQLITE_OK {
            return Err(db_error(self.db, "sqlite3_bind_text"));
        }
        Ok(())
    }

    fn bind_opt_text(&mut self, index: c_int, s: Option<&str>) -> io::Result<()> {
        match s {
            Some(v) => self.bind_text(index, v),
            None => {
                // SAFETY: `self.raw` is a live prepared statement.
                let rc = unsafe { sqlite3_bind_null(self.raw, index) };
                if rc != SQLITE_OK {
                    return Err(db_error(self.db, "sqlite3_bind_null"));
                }
                Ok(())
            }
        }
    }

    fn bind_i64(&mut self, index: c_int, val: i64) -> io::Result<()> {
        // SAFETY: `self.raw` is a live prepared statement.
        let rc = unsafe { sqlite3_bind_int64(self.raw, index, val) };
        if rc != SQLITE_OK {
            return Err(db_error(self.db, "sqlite3_bind_int64"));
        }
        Ok(())
    }

    fn bind_f64(&mut self, index: c_int, val: f64) -> io::Result<()> {
        // SAFETY: `self.raw` is a live prepared statement.
        let rc = unsafe { sqlite3_bind_double(self.raw, index, val) };
        if rc != SQLITE_OK {
            return Err(db_error(self.db, "sqlite3_bind_double"));
        }
        Ok(())
    }

    fn step_row(&mut self) -> io::Result<bool> {
        // SAFETY: `self.raw` is a live prepared statement bound to a live database handle.
        let rc = unsafe { sqlite3_step(self.raw) };
        match rc {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            _ => Err(db_error(self.db, "sqlite3_step")),
        }
    }

    fn step_done(&mut self) -> io::Result<()> {
        // SAFETY: `self.raw` is a live prepared statement bound to a live database handle.
        let rc = unsafe { sqlite3_step(self.raw) };
        if rc == SQLITE_DONE || rc == SQLITE_ROW {
            Ok(())
        } else {
            Err(db_error(self.db, "sqlite3_step"))
        }
    }

    fn reset(&mut self) {
        // SAFETY: `self.raw` is a live prepared statement.
        unsafe {
            sqlite3_reset(self.raw);
            sqlite3_clear_bindings(self.raw);
        }
    }

    fn col_text(&self, col: c_int) -> String {
        self.col_opt_text(col).unwrap_or_default()
    }

    fn col_opt_text(&self, col: c_int) -> Option<String> {
        // SAFETY: `self.raw` is positioned on a `SQLITE_ROW` result row.
        if unsafe { sqlite3_column_type(self.raw, col) } == SQLITE_NULL {
            return None;
        }
        // SAFETY: `self.raw` is positioned on a row; SQLite owns the returned buffer until next step/reset.
        let ptr = unsafe { sqlite3_column_text(self.raw, col) };
        // SAFETY: `sqlite3_column_bytes` returns the byte length of the column value returned above.
        let len = unsafe { sqlite3_column_bytes(self.raw, col) };
        if ptr.is_null() || len <= 0 {
            return Some(String::new());
        }
        // SAFETY: `ptr` is non-null and valid for `len` bytes during this borrow.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    fn col_i64(&self, col: c_int) -> i64 {
        // SAFETY: `self.raw` is positioned on a `SQLITE_ROW` result row.
        unsafe { sqlite3_column_int64(self.raw, col) }
    }

    fn col_f64(&self, col: c_int) -> f64 {
        // SAFETY: `self.raw` is positioned on a `SQLITE_ROW` result row.
        unsafe { sqlite3_column_double(self.raw, col) }
    }
}

fn like_pattern(needle: &str) -> String {
    let mut out = String::with_capacity(needle.len() + 2);
    out.push('%');
    for ch in needle.to_lowercase().chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

fn photo_mtime_ns(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn bind_catalog_item(stmt: &mut SqliteStmt<'_>, item: &CatalogItem) -> io::Result<()> {
    let cam_mul_json = serde_json::to_string(&item.metadata.cam_mul)
        .unwrap_or_else(|_| "[1.0,1.0,1.0,1.0]".to_string());
    stmt.bind_text(1, &item.path)?;
    stmt.bind_i64(2, photo_mtime_ns(&item.path))?;
    stmt.bind_i64(3, i64::from(item.rating.min(5)))?;
    stmt.bind_text(4, &item.flag)?;
    stmt.bind_text(5, &item.color_label)?;
    stmt.bind_text(6, &item.id)?;
    stmt.bind_opt_text(7, item.remote_path.as_deref())?;
    stmt.bind_text(8, &item.thumbnail_path)?;
    stmt.bind_text(9, &item.metadata.make)?;
    stmt.bind_text(10, &item.metadata.model)?;
    stmt.bind_text(11, &item.metadata.lens)?;
    stmt.bind_text(12, &item.recipe.film_simulation)?;
    stmt.bind_f64(13, f64::from(item.metadata.iso))?;
    stmt.bind_f64(14, f64::from(item.metadata.shutter))?;
    stmt.bind_f64(15, f64::from(item.metadata.aperture))?;
    stmt.bind_f64(16, f64::from(item.metadata.focal_length))?;
    stmt.bind_i64(17, i64::from(item.metadata.width))?;
    stmt.bind_i64(18, i64::from(item.metadata.height))?;
    stmt.bind_i64(19, i64::from(item.metadata.raw_width))?;
    stmt.bind_i64(20, i64::from(item.metadata.raw_height))?;
    stmt.bind_i64(21, item.metadata.timestamp)?;
    stmt.bind_text(22, &cam_mul_json)?;
    stmt.bind_text(23, "")?;
    stmt.bind_i64(24, item.updated_at.min(i64::MAX as u64) as i64)?;
    Ok(())
}

fn read_catalog_item(stmt: &SqliteStmt<'_>) -> CatalogItem {
    let path = stmt.col_text(0);
    let rating = stmt.col_i64(1).clamp(0, 5) as u8;
    let flag = stmt.col_text(2);
    let color_label = stmt.col_text(3);
    let id = stmt.col_text(4);
    let remote_path = stmt.col_opt_text(5);
    let thumbnail_path = stmt.col_text(6);
    let make = stmt.col_text(7);
    let model = stmt.col_text(8);
    let lens = stmt.col_text(9);
    let film_sim = stmt.col_text(10);
    let iso = stmt.col_f64(11) as f32;
    let shutter = stmt.col_f64(12) as f32;
    let aperture = stmt.col_f64(13) as f32;
    let focal_length = stmt.col_f64(14) as f32;
    let width = stmt.col_i64(15).max(0) as u32;
    let height = stmt.col_i64(16).max(0) as u32;
    let raw_width = stmt.col_i64(17).max(0) as u32;
    let raw_height = stmt.col_i64(18).max(0) as u32;
    let timestamp = stmt.col_i64(19);
    let cam_mul: [f32; 4] = serde_json::from_str(&stmt.col_text(20)).unwrap_or([1.0; 4]);
    let updated_at = stmt.col_i64(21).max(0) as u64;

    let recipe = Recipe {
        rating,
        flag: flag.clone(),
        film_simulation: if film_sim.is_empty() { "none".to_string() } else { film_sim },
        ..Recipe::default()
    };

    CatalogItem {
        id,
        path,
        remote_path,
        thumbnail_path,
        metadata: RawMetadata {
            width,
            height,
            raw_width,
            raw_height,
            make,
            model,
            lens,
            iso,
            shutter,
            aperture,
            focal_length,
            timestamp,
            cam_mul,
        },
        rating,
        color_label,
        flag,
        recipe,
        updated_at,
    }
}
