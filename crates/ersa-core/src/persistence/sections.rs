//! Sections: the groups the user makes for sessions in the rail (ADR 0026).
//!
//! A section's name is unique ignoring case. The caller passes each name
//! with its key, the name as sections are compared, and the table's unique
//! key on it refuses a second section with the same key, two requests
//! racing included, with [`super::Error::Conflict`].

use ersa_protocol::core_api::Section;
use rusqlite::{OptionalExtension, Row, params};

use super::{Result, Store, conflict, new_id, now_rfc3339};

const TAKEN: &str = "another section already has that name";

impl Store {
    /// Every section, in the order they were made.
    pub async fn sections(&self) -> Result<Vec<Section>> {
        self.call(|conn| {
            let mut stmt = conn.prepare(
                // `rowid` breaks a tie between two made in one millisecond.
                "SELECT id, name, created_at FROM sections ORDER BY created_at, rowid",
            )?;
            let rows = stmt.query_map([], section_row)?;
            super::collect(rows)
        })
        .await
    }

    /// Makes a section. Fails with [`super::Error::Conflict`] when another
    /// section has `key`.
    pub async fn insert_section(&self, name: String, key: String) -> Result<Section> {
        self.call(move |conn| {
            let section = Section {
                id: new_id("sec"),
                name,
                created_at: now_rfc3339(),
            };
            conn.execute(
                "INSERT INTO sections (id, name, name_key, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![section.id, section.name, key, section.created_at],
            )
            .map_err(|e| conflict(e, TAKEN))?;
            Ok(section)
        })
        .await
    }

    /// Renames section `id`; `None` when there is no such section. Fails
    /// with [`super::Error::Conflict`] when another section has `key`.
    pub async fn rename_section(
        &self,
        id: String,
        name: String,
        key: String,
    ) -> Result<Option<Section>> {
        self.call(move |conn| {
            conn.query_row(
                "UPDATE sections SET name = ?2, name_key = ?3 WHERE id = ?1
                 RETURNING id, name, created_at",
                params![id, name, key],
                section_row,
            )
            .optional()
            .map_err(|e| conflict(e, TAKEN))
        })
        .await
    }

    /// Removes section `id` and puts its sessions in no section, in one
    /// transaction. Returns the section as it was; `None`, changing
    /// nothing, when there is no such section.
    pub async fn delete_section(&self, id: String) -> Result<Option<Section>> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE sessions SET section_id = NULL WHERE section_id = ?1",
                [&id],
            )?;
            let removed = tx
                .query_row(
                    "DELETE FROM sections WHERE id = ?1 RETURNING id, name, created_at",
                    [&id],
                    section_row,
                )
                .optional()?;
            tx.commit()?;
            Ok(removed)
        })
        .await
    }
}

fn section_row(row: &Row) -> rusqlite::Result<Section> {
    Ok(Section {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
    })
}
