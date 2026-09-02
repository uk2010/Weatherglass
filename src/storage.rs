use crate::models::SavedLocation;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocationStore {
    path: PathBuf,
}

impl LocationStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn xdg() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("io", "Weatherglass", "Weatherglass")
            .context("XDG data directory unavailable")?;
        Ok(Self::new(dirs.data_dir().join("weatherglass.db")))
    }
    pub async fn migrate(&self) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || migrate_sync(&p)).await??;
        Ok(())
    }
    pub async fn list(&self) -> Result<Vec<SavedLocation>> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || list_sync(&p)).await?
    }
    pub async fn upsert(&self, location: SavedLocation) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || upsert_sync(&p, &location)).await??;
        Ok(())
    }
    pub async fn delete(&self, id: String) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || delete_sync(&p, &id)).await??;
        Ok(())
    }
    pub async fn rename(&self, id: String, name: String) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let c = open(&p)?;
            c.execute(
                "UPDATE locations SET display_name=?1 WHERE id=?2",
                params![name, id],
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        Ok(())
    }
    pub async fn reorder(&self, ids: Vec<String>) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || reorder_sync(&p, &ids)).await??;
        Ok(())
    }
    pub async fn select(&self, id: String) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || select_sync(&p, &id)).await??;
        Ok(())
    }
}

fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let c = Connection::open(path)?;
    c.pragma_update(None, "foreign_keys", "ON")?;
    Ok(c)
}
fn migrate_sync(path: &Path) -> Result<()> {
    let mut c = open(path)?;
    let tx = c.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS schema_version(version INTEGER NOT NULL); INSERT INTO schema_version(version) SELECT 0 WHERE NOT EXISTS(SELECT 1 FROM schema_version);")?;
    let version: i64 = tx.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;
    if version < 1 {
        tx.execute_batch("CREATE TABLE locations(id TEXT PRIMARY KEY NOT NULL,display_name TEXT NOT NULL,country_code TEXT NOT NULL,timezone TEXT NOT NULL,latitude REAL NOT NULL CHECK(latitude BETWEEN -90 AND 90),longitude REAL NOT NULL CHECK(longitude BETWEEN -180 AND 180),sort_order INTEGER NOT NULL,last_selected INTEGER NOT NULL DEFAULT 0,UNIQUE(latitude,longitude)); CREATE INDEX locations_order ON locations(sort_order); UPDATE schema_version SET version=1;")?;
    }
    tx.commit()?;
    Ok(())
}
fn list_sync(path: &Path) -> Result<Vec<SavedLocation>> {
    let c = open(path)?;
    let mut s=c.prepare("SELECT id,display_name,country_code,timezone,latitude,longitude,sort_order,last_selected FROM locations ORDER BY sort_order,id")?;
    Ok(s.query_map([], |r| {
        Ok(SavedLocation {
            id: r.get(0)?,
            display_name: r.get(1)?,
            country_code: r.get(2)?,
            timezone: r.get(3)?,
            latitude: r.get(4)?,
            longitude: r.get(5)?,
            sort_order: r.get(6)?,
            last_selected: r.get::<_, i64>(7)? != 0,
        })
    })?
    .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn upsert_sync(path: &Path, l: &SavedLocation) -> Result<()> {
    let c = open(path)?;
    let next: i64 = c.query_row(
        "SELECT COALESCE(MAX(sort_order),-1)+1 FROM locations",
        [],
        |r| r.get(0),
    )?;
    c.execute("INSERT INTO locations(id,display_name,country_code,timezone,latitude,longitude,sort_order,last_selected) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name,country_code=excluded.country_code,timezone=excluded.timezone,latitude=excluded.latitude,longitude=excluded.longitude,sort_order=excluded.sort_order,last_selected=excluded.last_selected",params![l.id,l.display_name,l.country_code,l.timezone,l.latitude,l.longitude,if l.sort_order<0{next}else{l.sort_order},l.last_selected as i64])?;
    Ok(())
}
fn delete_sync(path: &Path, id: &str) -> Result<()> {
    let mut c = open(path)?;
    let tx = c.transaction()?;
    let was: Option<i64> = tx
        .query_row(
            "SELECT last_selected FROM locations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    tx.execute("DELETE FROM locations WHERE id=?1", [id])?;
    if was == Some(1) {
        tx.execute("UPDATE locations SET last_selected=1 WHERE id=(SELECT id FROM locations ORDER BY sort_order LIMIT 1)",[])?;
    }
    tx.execute("UPDATE locations SET sort_order=(SELECT COUNT(*) FROM locations b WHERE b.sort_order<locations.sort_order)",[])?;
    tx.commit()?;
    Ok(())
}
fn reorder_sync(path: &Path, ids: &[String]) -> Result<()> {
    let mut c = open(path)?;
    let tx = c.transaction()?;
    for (n, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE locations SET sort_order=?1 WHERE id=?2",
            params![n as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}
fn select_sync(path: &Path, id: &str) -> Result<()> {
    let mut c = open(path)?;
    let tx = c.transaction()?;
    tx.execute("UPDATE locations SET last_selected=0", [])?;
    tx.execute("UPDATE locations SET last_selected=1 WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn persistence_order_dedup_delete_and_migration() {
        let t = tempfile::tempdir().unwrap();
        let s = LocationStore::new(t.path().join("x.db"));
        s.migrate().await.unwrap();
        s.migrate().await.unwrap();
        let mut a = SavedLocation::new("A", "US", "America/Chicago", 1., 2.);
        a.sort_order = 0;
        a.last_selected = true;
        let mut b = SavedLocation::new("B", "GB", "Europe/London", 3., 4.);
        b.sort_order = 1;
        s.upsert(a.clone()).await.unwrap();
        s.upsert(b.clone()).await.unwrap();
        s.reorder(vec![b.id.clone(), a.id.clone()]).await.unwrap();
        assert_eq!(s.list().await.unwrap()[0].id, b.id);
        s.select(b.id.clone()).await.unwrap();
        s.delete(b.id).await.unwrap();
        let got = s.list().await.unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].last_selected);
    }
    #[tokio::test]
    async fn coordinate_deduplication() {
        let t = tempfile::tempdir().unwrap();
        let s = LocationStore::new(t.path().join("x.db"));
        s.migrate().await.unwrap();
        s.upsert(SavedLocation::new("A", "US", "Etc/UTC", 1., 2.))
            .await
            .unwrap();
        let e = s
            .upsert(SavedLocation::new("Again", "US", "Etc/UTC", 1., 2.))
            .await;
        assert!(e.is_err());
    }
}
