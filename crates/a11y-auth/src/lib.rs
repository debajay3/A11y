//! Local server-owned accounts. No password or session secret implements Debug.
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub admin: bool,
    pub active: bool,
    pub must_change_password: bool,
    /// Default shared-library access: none, viewer, editor. Explicit grants can add access.
    pub access: String,
}
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub user: User,
    pub expires: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant {
    pub user_id: String,
    pub kind: String,
    pub resource: String,
    pub role: String,
}

pub struct Store {
    db: Connection,
    dummy: String,
}
fn db_error(_: rusqlite::Error) -> String {
    "Account database operation failed".into()
}
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}
pub fn secret() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Random generator unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn digest(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(password: &str) -> Result<String> {
    if password.len() < 14 || password.len() > 256 {
        return Err("Use a password of 14–256 bytes".into());
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| "Random generator unavailable")?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| "Cannot create password salt")?;
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|_| "Cannot hash password".into())
}
fn verify(password: &str, encoded: &str) -> bool {
    password.len() <= 256 && PasswordHash::new(encoded).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}
fn username(value: &str) -> Result<String> {
    let s = value.trim().to_ascii_lowercase();
    if s.is_empty() || s.len() > 128 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._-+".contains(&b)) {
        return Err("Use letters, numbers, @, dot, dash or underscore in usernames".into());
    }
    Ok(s)
}
fn role(s: &str) -> Result<()> {
    if matches!(s, "none" | "viewer" | "editor") {
        Ok(())
    } else {
        Err("Invalid access level".into())
    }
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User { id: r.get(0)?, username: r.get(1)?, admin: r.get(2)?, active: r.get(3)?, must_change_password: r.get(4)?, access: r.get(5)? })
}
const SELECT: &str = "SELECT id,username,admin,active,must_change,access FROM users";
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path).map_err(db_error)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_error)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
CREATE TABLE IF NOT EXISTS users(id TEXT PRIMARY KEY, username TEXT UNIQUE NOT NULL, password TEXT NOT NULL, admin INTEGER NOT NULL, active INTEGER NOT NULL DEFAULT 1, must_change INTEGER NOT NULL DEFAULT 1, access TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions(digest TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES users(id),expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS attempts(key TEXT PRIMARY KEY, failures INTEGER NOT NULL, until_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS grants(user_id TEXT NOT NULL REFERENCES users(id),kind TEXT NOT NULL,resource TEXT NOT NULL,role TEXT NOT NULL,PRIMARY KEY(user_id,kind,resource));
CREATE TABLE IF NOT EXISTS audit(at INTEGER NOT NULL,actor TEXT NOT NULL,action TEXT NOT NULL,target TEXT NOT NULL);").map_err(db_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|_| "Cannot protect accounts database")?;
        }
        Ok(Self { db, dummy: hash("unusable dummy password for timing")? })
    }
    fn audit(&self, actor: &str, action: &str, target: &str) -> Result<()> {
        self.db.execute("INSERT INTO audit VALUES(?,?,?,?)", params![now(), actor, action, target]).map_err(db_error)?;
        Ok(())
    }
    pub fn bootstrap(&mut self, name: &str, password: &str) -> Result<User> {
        let tx = self.db.transaction().map_err(db_error)?;
        if tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0)).map_err(db_error)? != 0 {
            return Err("Administrator setup has already completed".into());
        }
        let id = secret()?;
        let name = username(name)?;
        let encoded = hash(password)?;
        tx.execute("INSERT INTO users(id,username,password,admin,access) VALUES(?,?,?,1,'editor')", params![id, name, encoded]).map_err(db_error)?;
        tx.execute("INSERT INTO audit VALUES(?,'local-setup','bootstrap',?)", params![now(), id]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.user(&id)
    }
    pub fn user(&self, id: &str) -> Result<User> {
        self.db.query_row(&format!("{SELECT} WHERE id=?"), [id], row).map_err(db_error)
    }
    pub fn authenticate(&self, token: &str) -> Result<User> {
        if token.len() != 64 {
            return Err("Sign in to continue".into());
        }
        self.db
            .query_row(
                &format!("{SELECT} WHERE active=1 AND id=(SELECT user_id FROM sessions WHERE digest=? AND expires>?)"),
                params![digest(token), now()],
                row,
            )
            .map_err(|_| "Your session expired or was revoked. Sign in again.".into())
    }
    pub fn ready(&self, token: &str) -> Result<User> {
        let u = self.authenticate(token)?;
        if u.must_change_password {
            Err("Change your temporary password first".into())
        } else {
            Ok(u)
        }
    }
    pub fn admin(&self, token: &str) -> Result<User> {
        let u = self.ready(token)?;
        if u.admin {
            Ok(u)
        } else {
            Err("Administrator access required".into())
        }
    }
    fn session(&self, user: User) -> Result<Session> {
        let token = secret()?;
        let expires = now() + if user.must_change_password { 600 } else { 7 * 24 * 3600 };
        self.db.execute("DELETE FROM sessions WHERE expires<=?", [now()]).map_err(db_error)?;
        self.db.execute("INSERT INTO sessions VALUES(?,?,?)", params![digest(&token), user.id, expires]).map_err(db_error)?;
        Ok(Session { token, user, expires })
    }
    pub fn login(&self, name: &str, password: &str, source: &str) -> Result<Session> {
        let normalized = name.trim().to_ascii_lowercase();
        let keys = [digest(&format!("user:{normalized}")), digest(&format!("source:{source}"))];
        for k in &keys {
            let count = self
                .db
                .query_row("SELECT failures FROM attempts WHERE key=? AND until_at>?", params![k, now()], |r| r.get::<_, u32>(0))
                .optional()
                .map_err(db_error)?
                .unwrap_or(0);
            if count >= 10 {
                return Err("Too many sign-in attempts. Try again in 15 minutes.".into());
            }
        }
        let found = self
            .db
            .query_row("SELECT id,password FROM users WHERE username=? AND active=1", [&normalized], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .optional()
            .map_err(db_error)?;
        let valid = verify(password, found.as_ref().map_or(self.dummy.as_str(), |(_, h)| h));
        if !valid || found.is_none() {
            self.db.execute("DELETE FROM attempts WHERE until_at<=?", [now()]).map_err(db_error)?;
            for k in keys {
                self.db
                    .execute("INSERT INTO attempts VALUES(?,1,?) ON CONFLICT(key) DO UPDATE SET failures=failures+1", params![k, now() + 900])
                    .map_err(db_error)?;
            }
            return Err("Incorrect username or password".into());
        }
        self.db.execute("DELETE FROM attempts WHERE key=?", [&keys[0]]).map_err(db_error)?;
        let user = self.user(&found.unwrap().0)?;
        self.audit(&user.id, "login", &user.id)?;
        self.session(user)
    }
    pub fn logout(&self, token: &str) -> Result<()> {
        self.db.execute("DELETE FROM sessions WHERE digest=?", [digest(token)]).map_err(db_error)?;
        Ok(())
    }
    pub fn change_password(&mut self, token: &str, old: &str, new: &str) -> Result<Session> {
        let u = self.authenticate(token)?;
        let encoded: String = self.db.query_row("SELECT password FROM users WHERE id=?", [&u.id], |r| r.get(0)).map_err(db_error)?;
        if !verify(old, &encoded) {
            return Err("Current password is incorrect".into());
        }
        if verify(new, &encoded) {
            return Err("Choose a different password".into());
        }
        let encoded = hash(new)?;
        let tx = self.db.transaction().map_err(db_error)?;
        tx.execute("UPDATE users SET password=?,must_change=0 WHERE id=?", params![encoded, u.id]).map_err(db_error)?;
        tx.execute("DELETE FROM sessions WHERE user_id=?", [&u.id]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.audit(&u.id, "password-change", &u.id)?;
        self.session(self.user(&u.id)?)
    }
    pub fn list(&self, token: &str) -> Result<Vec<User>> {
        self.admin(token)?;
        self.db
            .prepare(&format!("{SELECT} ORDER BY username"))
            .map_err(db_error)?
            .query_map([], row)
            .map_err(db_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db_error)
    }
    pub fn create(&mut self, token: &str, name: &str, password: &str, admin: bool, access: &str) -> Result<User> {
        let actor = self.admin(token)?;
        let name = username(name)?;
        role(access)?;
        let encoded = hash(password)?;
        let id = secret()?;
        self.db
            .execute("INSERT INTO users(id,username,password,admin,access) VALUES(?,?,?,?,?)", params![id, name, encoded, admin, access])
            .map_err(|_| "Cannot create account; username may already exist")?;
        self.audit(&actor.id, "user-create", &id)?;
        self.user(&id)
    }
    pub fn update(&mut self, token: &str, id: &str, active: bool, admin: bool, access: &str) -> Result<User> {
        let actor = self.admin(token)?;
        role(access)?;
        self.user(id)?;
        let tx = self.db.transaction().map_err(db_error)?;
        tx.execute("UPDATE users SET active=?,admin=?,access=? WHERE id=?", params![active, admin, access, id]).map_err(db_error)?;
        let n: i64 = tx.query_row("SELECT COUNT(*) FROM users WHERE active=1 AND admin=1", [], |r| r.get(0)).map_err(db_error)?;
        if n == 0 {
            return Err("Keep at least one active administrator".into());
        }
        tx.execute("DELETE FROM sessions WHERE user_id=?", [id]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.audit(&actor.id, "user-update", id)?;
        self.user(id)
    }
    pub fn reset(&mut self, token: &str, id: &str, password: &str) -> Result<()> {
        let actor = self.admin(token)?;
        self.user(id)?;
        let encoded = hash(password)?;
        let tx = self.db.transaction().map_err(db_error)?;
        tx.execute("UPDATE users SET password=?,must_change=1 WHERE id=?", params![encoded, id]).map_err(db_error)?;
        tx.execute("DELETE FROM sessions WHERE user_id=?", [id]).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.audit(&actor.id, "password-reset", id)
    }
    pub fn grants(&self, token: &str) -> Result<Vec<Grant>> {
        self.admin(token)?;
        self.db
            .prepare("SELECT user_id,kind,resource,role FROM grants ORDER BY user_id,kind,resource")
            .map_err(db_error)?
            .query_map([], |r| Ok(Grant { user_id: r.get(0)?, kind: r.get(1)?, resource: r.get(2)?, role: r.get(3)? }))
            .map_err(db_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db_error)
    }
    pub fn grant(&self, token: &str, g: &Grant) -> Result<()> {
        let actor = self.admin(token)?;
        self.user(&g.user_id)?;
        role(&g.role)?;
        if !matches!(g.kind.as_str(), "file" | "project") || g.resource.is_empty() || g.resource.len() > 128 {
            return Err("Invalid resource".into());
        }
        self.db
            .execute(
                "INSERT INTO grants VALUES(?,?,?,?) ON CONFLICT(user_id,kind,resource) DO UPDATE SET role=excluded.role",
                params![g.user_id, g.kind, g.resource, g.role],
            )
            .map_err(db_error)?;
        self.db.execute("DELETE FROM sessions WHERE user_id=?", [&g.user_id]).map_err(db_error)?;
        self.audit(&actor.id, "grant-update", &g.user_id)
    }
    pub fn access(&self, token: &str, file: &str, project: &str) -> Result<String> {
        let u = self.ready(token)?;
        // Initial single-workspace admin explicitly owns all existing content.
        if u.admin {
            return Ok("editor".into());
        }
        // Explicit file/project rules override the workspace default, including "none".
        for (kind, resource) in [("file", file), ("project", project)] {
            if let Some(role) = self
                .db
                .query_row("SELECT role FROM grants WHERE user_id=? AND kind=? AND resource=?", params![u.id, kind, resource], |r| r.get::<_, String>(0))
                .optional()
                .map_err(db_error)?
            {
                return Ok(role);
            }
        }
        Ok(u.access)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Store, Session, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("a11y-auth-{}.db", secret().unwrap()));
        let mut s = Store::open(&p).unwrap();
        s.bootstrap("Admin", "temporary-password-123").unwrap();
        let a = s.login("ADMIN", "temporary-password-123", "local").unwrap();
        assert!(s.admin(&a.token).is_err());
        let a = s.change_password(&a.token, "temporary-password-123", "permanent-admin-password-456").unwrap();
        (s, a, p)
    }
    #[test]
    fn lifecycle_and_revocation() {
        let (mut s, a, p) = setup();
        assert!(s.bootstrap("other", "some-long-password").is_err());
        let u = s.create(&a.token, "designer", "initial-designer-password", false, "viewer").unwrap();
        let session = s.login("designer", "initial-designer-password", "local").unwrap();
        assert!(s.ready(&session.token).is_err());
        let session = s.change_password(&session.token, "initial-designer-password", "new-designer-password").unwrap();
        assert!(s.list(&session.token).is_err());
        assert_eq!(s.access(&session.token, "file", "project").unwrap(), "viewer");
        s.grant(&a.token, &Grant { user_id: u.id.clone(), kind: "file".into(), resource: "file".into(), role: "none".into() }).unwrap();
        assert!(s.access(&session.token, "file", "project").is_err());
        let session = s.login("designer", "new-designer-password", "local").unwrap();
        assert_eq!(s.access(&session.token, "file", "project").unwrap(), "none");
        s.reset(&a.token, &u.id, "replacement-password-123").unwrap();
        assert!(s.ready(&session.token).is_err());
        assert!(s.update(&a.token, &a.user.id, false, false, "none").is_err());
        assert!(s.admin(&a.token).is_ok());
        drop(s);
        let s = Store::open(&p).unwrap();
        assert!(s.admin(&a.token).is_ok());
        drop(s);
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn rejects_bad_credentials_and_throttles() {
        let (s, a, p) = setup();
        assert!(s.authenticate("forged").is_err());
        for _ in 0..10 {
            assert!(s.login("missing", "incorrect-password-123", "source").is_err());
        }
        assert!(s.login("admin", "permanent-admin-password-456", "source").unwrap_err_text().contains("Too many"));
        s.logout(&a.token).unwrap();
        assert!(s.ready(&a.token).is_err());
        drop(s);
        let _ = std::fs::remove_file(p);
    }
    trait ErrorText {
        fn unwrap_err_text(self) -> String;
    }
    impl<T> ErrorText for Result<T> {
        fn unwrap_err_text(self) -> String {
            match self {
                Err(e) => e,
                Ok(_) => panic!("expected rejection"),
            }
        }
    }
}
