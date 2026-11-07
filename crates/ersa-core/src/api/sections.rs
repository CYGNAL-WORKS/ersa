//! The `section.*` methods: the sections the user groups sessions in
//! (PROTO-CORE-API, Sections; FLOW-ORGANIZE-SESSIONS).

use ersa_protocol::core_api::{Empty, IdParams, Section, SectionCreateParams, SectionRenameParams};

use super::ApiError;
use crate::persistence;
use crate::state::Core;

/// The most characters a section's name may have, once trimmed.
const MAX_NAME_CHARS: usize = 80;

pub(super) async fn list(core: &Core, _: Empty) -> Result<Vec<Section>, ApiError> {
    Ok(core.store.sections().await?)
}

pub(super) async fn create(core: &Core, params: SectionCreateParams) -> Result<Section, ApiError> {
    let (name, key) = checked_name(&params.name)?;
    core.store
        .insert_section(name, key)
        .await
        .map_err(name_taken)
}

pub(super) async fn rename(core: &Core, params: SectionRenameParams) -> Result<Section, ApiError> {
    let (name, key) = checked_name(&params.name)?;
    core.store
        .rename_section(params.id.clone(), name, key)
        .await
        .map_err(name_taken)?
        .ok_or_else(|| ApiError::not_found(format!("no section {}", params.id)))
}

/// Removes the section and puts its sessions in no section
/// (PROTO-CORE-API-R39); answers with the section as it was.
pub(super) async fn remove(core: &Core, params: IdParams) -> Result<Section, ApiError> {
    core.sessions
        .remove_section(&params.id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no section {}", params.id)))
}

/// The name as one line, as a session's title is taken, and its key: the
/// name as sections are compared, ignoring case. Refuses a name holding a
/// control character other than spacing, and one that is then blank or
/// longer than 80 characters (FLOW-ORGANIZE-SESSIONS-R06). Characters, not
/// bytes: a name in an accented or non-Latin script is as long as it reads.
fn checked_name(name: &str) -> Result<(String, String), ApiError> {
    let Some(name) = super::sessions::one_line(name) else {
        return Err(ApiError::invalid_params(
            "a section's name is one line, without control characters",
        ));
    };
    if name.is_empty() {
        return Err(ApiError::invalid_params("a section's name cannot be blank"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::invalid_params(format!(
            "a section's name can have at most {MAX_NAME_CHARS} characters"
        )));
    }
    // `to_lowercase` folds case beyond ASCII ("É" and "é" are one name).
    let key = name.to_lowercase();
    Ok((name, key))
}

/// The store refuses a name another section has, ignoring case, as a
/// conflict on the name's key: that is the caller's params, not Core's
/// state, so it answers `invalid_params` (FLOW-ORGANIZE-SESSIONS-R06).
fn name_taken(e: persistence::Error) -> ApiError {
    match e {
        persistence::Error::Conflict(message) => ApiError::invalid_params(message),
        other => other.into(),
    }
}
