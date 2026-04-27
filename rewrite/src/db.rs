use rusqlite::Connection;

#[derive(Debug)]
pub enum DbError {
    DuplicateLabel(String),
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE labels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn insert_label(&self, name: &str) -> Result<()> {
        match self
            .conn
            .execute("INSERT INTO labels (name) VALUES (?1)", [name])
        {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(DbError::DuplicateLabel(name.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    pub fn count_labels(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM labels", [], |row| row.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserting_label_increases_count() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_two_distinct_labels_yields_count_of_two() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn inserting_duplicate_label_returns_err() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let second = db.insert_label("Morning");
        assert!(second.is_err(), "second insert of same label should fail");
        // The first insert is preserved; no duplicate row is created.
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn duplicate_label_error_identifies_offending_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let err = db.insert_label("Morning").unwrap_err();
        assert!(
            matches!(err, DbError::DuplicateLabel(ref name) if name == "Morning"),
            "expected DuplicateLabel(\"Morning\"), got {err:?}"
        );
    }
}
