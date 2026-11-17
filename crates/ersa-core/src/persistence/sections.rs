//! Sections: the groups the user makes for sessions in the rail (ADR 0026).
//!
//! A section's name is unique, compared exactly: names that differ only in
//! case are different sections (PROTO-CORE-API-R76). The table's unique
//! `name_key` holds the name, so two requests racing never make two
//! sections with one name.

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

    /// Makes a section named `name`, or finds the one that has it already.
    /// `made` runs with a
    /// section this call made, before the connection is free for another
    /// call, so whatever it does comes before any later call finds the
    /// section: a caller that tells others of the section there never
    /// answers with one they have not been told of.
    ///
    /// [`super::Error::Conflict`] is not possible here: a taken name finds
    /// its section.
    pub async fn insert_section(
        &self,
        name: String,
        made: impl FnOnce(&Section) + Send + 'static,
    ) -> Result<Section> {
        self.call(move |conn| {
            let inserted = conn
                .query_row(
                    // `DO NOTHING` returns no row when the name is taken.
                    "INSERT INTO sections (id, name, name_key, created_at) VALUES (?1, ?2, ?2, ?3)
                     ON CONFLICT (name_key) DO NOTHING
                     RETURNING id, name, created_at",
                    params![new_id("sec"), name, now_rfc3339()],
                    section_row,
                )
                .optional()?;
            if let Some(section) = inserted {
                made(&section);
                return Ok(section);
            }
            let found = conn.query_row(
                "SELECT id, name, created_at FROM sections WHERE name_key = ?1",
                [&name],
                section_row,
            )?;
            Ok(found)
        })
        .await
    }

    /// Renames section `id`; `None` when there is no such section. Fails
    /// with [`super::Error::Conflict`] when another section has `name`.
    pub async fn rename_section(&self, id: String, name: String) -> Result<Option<Section>> {
        self.call(move |conn| {
            conn.query_row(
                "UPDATE sections SET name = ?2, name_key = ?2 WHERE id = ?1
                 RETURNING id, name, created_at",
                params![id, name],
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
