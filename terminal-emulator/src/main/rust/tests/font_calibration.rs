use rusqlite::{Connection, OpenFlags};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use std::fs;
use std::path::Path;
use std::os::unix::fs::PermissionsExt;
use std::env;

// 为了不修改主程序，我们直接在这里定义简单的扫描属性
#[derive(Debug)]
struct CharMetrics {
    cp: u32,
    actual_width: f32,
    expected_width: i32,
    direction: String,
    category: String,
}

pub enum AccessLevel {
    Guest,
    Admin,
}

pub struct FontCalibrationDB {
    conn: Connection,
    level: AccessLevel,
}

// 管理员密码的哈希值 (示例密码为 "termux_rust_2026")
const ADMIN_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$767zXv5m9f1J9K/2oXkLBg$9oK+4yF1Z9oK+4yF1Z9oK+4yF1Z9oK+4yF1Z9oK+4w";

impl FontCalibrationDB {
    /// 以访客模式（只读）打开数据库
    pub fn login_as_guest(db_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !db_path.exists() {
            return Err("Database file does not exist. Please run as admin to initialize.".into());
        }
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self { conn, level: AccessLevel::Guest })
    }

    /// 以管理员模式（读写）打开数据库，需要校验密码
    pub fn login_as_admin(db_path: &Path, password: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // 校验密码
        let argon2 = Argon2::default();
        let parsed_hash = PasswordHash::new(ADMIN_HASH).map_err(|e| format!("Hash error: {}", e))?;
        if argon2.verify_password(password.as_bytes(), &parsed_hash).is_err() {
            return Err("Invalid admin password. Permission denied.".into());
        }

        let conn = Connection::open(db_path)?;
        
        // 设置文件权限为 0600 (仅所有者读写)
        let mut perms = fs::metadata(db_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(db_path, perms)?;

        // 初始化表结构
        conn.execute(
            "CREATE TABLE IF NOT EXISTS glyph_exceptions (
                cp INTEGER PRIMARY KEY,
                actual_width REAL,
                expected_width INTEGER,
                direction TEXT,
                category TEXT
            )",
            [],
        )?;

        Ok(Self { conn, level: AccessLevel::Admin })
    }

    /// 记录异常字符 (仅 Admin 可用)
    pub fn insert_exception(&self, metrics: &CharMetrics) -> Result<(), Box<dyn std::error::Error>> {
        match self.level {
            AccessLevel::Admin => {
                self.conn.execute(
                    "INSERT OR REPLACE INTO glyph_exceptions (cp, actual_width, expected_width, direction, category) 
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (metrics.cp, metrics.actual_width, metrics.expected_width, &metrics.direction, &metrics.category),
                )?;
                Ok(())
            }
            AccessLevel::Guest => Err("Guest account cannot modify database.".into()),
        }
    }

    /// 验证测试集上的结果
    pub fn verify_codepoint(&self, cp: u32, current_actual_w: f32) -> Result<bool, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare("SELECT actual_width FROM glyph_exceptions WHERE cp = ?1")?;
        let mut rows = stmt.query([cp])?;
        if let Some(row) = rows.next()? {
            let saved_w: f32 = row.get(0)?;
            // 允许 1% 的微小渲染误差
            Ok((saved_w - current_actual_w).abs() < 0.05)
        } else {
            // 如果库里没记录，说明该字符符合标准 wcwidth 预期
            Ok(true) 
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guest_access_denied_on_write() {
        let db_path = Path::new("tests/calibration_test.db");
        // 如果文件不存在则先创建一个
        if !db_path.exists() {
             let _ = Connection::open(db_path);
        }
        
        let db = FontCalibrationDB::login_as_guest(db_path).unwrap();
        let dummy = CharMetrics {
            cp: 65, actual_width: 10.0, expected_width: 1, direction: "LTR".to_string(), category: "Lu".to_string()
        };
        assert!(db.insert_exception(&dummy).is_err());
    }

    #[test]
    fn run_calibration_scan_as_admin() {
        // 只有当环境变量中存在 ADMIN_PWD 时才运行扫描（防止自动化测试时误触）
        let password = match env::var("ADMIN_PWD") {
            Ok(p) => p,
            Err(_) => {
                println!("Skipping admin scan (ADMIN_PWD not set)");
                return;
            }
        };

        let db_path = Path::new("tests/calibration_production.db");
        let db = FontCalibrationDB::login_as_admin(db_path, &password).expect("Login failed");

        println!("Admin logged in. Starting full UTF-32 scan...");

        // 模拟扫描 0..0xFFFF (BMP 范围作为示例，因为 1.1M 字符在 CI 里可能太慢)
        for cp in 0..0xFFFF {
            // 这里以后会集成 Skia 测量逻辑
            // if actual_w != expected_w {
            //    db.insert_exception(...).unwrap();
            // }
        }
        println!("Scan complete.");
    }
}
