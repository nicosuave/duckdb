pub struct Session {
    pub db: duckdb_sys::duckdb_database,
    pub con: duckdb_sys::duckdb_connection,
}

impl Session {
    pub fn new(db: duckdb_sys::duckdb_database, con: duckdb_sys::duckdb_connection) -> Self {
        Self { db, con }
    }
}
