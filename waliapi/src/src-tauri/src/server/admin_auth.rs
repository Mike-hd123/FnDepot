//! Web 管理面板认证：admin_users 表 + argon2id 哈希 + Bearer token session。

use std::sync::Arc;
use std::time::{Duration, Instant};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use dashmap::DashMap;
use password_hash::rand_core::OsRng;
use rand::{distr::Alphanumeric, Rng};
use sqlx::SqlitePool;

const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
pub struct AdminSession {
    pub user_id: String,
    pub username: String,
    pub must_change_password: bool,
}

#[derive(Default)]
pub struct SessionStore {
    inner: DashMap<String, (AdminSession, Instant)>,
}

impl SessionStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn insert(&self, token: String, session: AdminSession) {
        self.inner.insert(token, (session, Instant::now() + SESSION_TTL));
    }

    pub fn get(&self, token: &str) -> Option<AdminSession> {
        let entry = self.inner.get(token)?;
        let (session, expiry) = entry.value();
        if Instant::now() > *expiry {
            drop(entry);
            self.inner.remove(token);
            return None;
        }
        Some(session.clone())
    }

    pub fn remove(&self, token: &str) {
        self.inner.remove(token);
    }
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("密码哈希失败: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn generate_password(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub struct AdminUserRow {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub must_change_password: bool,
}

pub async fn find_user_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<AdminUserRow>, String> {
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, must_change_password FROM admin_users WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|(id, username, password_hash, mcp)| AdminUserRow {
        id,
        username,
        password_hash,
        must_change_password: mcp != 0,
    }))
}

pub async fn update_password(pool: &SqlitePool, user_id: &str, new_password: &str) -> Result<(), String> {
    let hash = hash_password(new_password)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE admin_users SET password_hash = ?, must_change_password = 0, updated_at = ? WHERE id = ?")
        .bind(hash)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_username(pool: &SqlitePool, user_id: &str, new_username: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE admin_users SET username = ?, updated_at = ? WHERE id = ?")
        .bind(new_username)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 首次启动时若无 admin 用户，创建 admin + 随机 16 位密码并写入 INITIAL_PASSWORD 文件。
/// `data_dir` 为应用数据目录（容器内 /data/<identifier>）。
pub async fn ensure_initial_admin(pool: &SqlitePool, data_dir: &std::path::Path) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admin_users")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }

    let username = "admin";
    let password = generate_password(16);
    let hash = hash_password(&password)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO admin_users (id, username, password_hash, must_change_password, created_at, updated_at) VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(username)
    .bind(&hash)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 临时密码必须出现在容器 stdout（docker logs），同时写入数据目录文件
    println!("==============================================");
    println!("WaLiAPI Web 管理面板初始账号: {username}");
    println!("初始临时密码: {password}");
    println!("请登录后立即修改密码。");
    println!("==============================================");

    // 写入数据目录，容器内为 /data/<identifier>/INITIAL_PASSWORD
    if let Err(e) = std::fs::create_dir_all(data_dir) {
        log::warn!("创建数据目录失败: {e}");
    }
    let file = data_dir.join("INITIAL_PASSWORD");
    if let Err(e) = std::fs::write(&file, format!("username: {username}\npassword: {password}\n")) {
        log::warn!("写入 INITIAL_PASSWORD 失败: {e}");
    } else {
        log::info!("初始密码已写入 {}", file.display());
    }

    Ok(())
}
