//! The `section.*` methods: the sections the user groups sessions in
//! (PROTO-CORE-API, Sections; FLOW-ORGANIZE-SESSIONS).

use ersa_protocol::core_api::{Empty, IdParams, Section, SectionCreateParams, SectionRenameParams};

use super::ApiError;
use crate::admission::Report;
use crate::persistence;
use crate::sessions::SessionError;
use crate::state::Core;

/// The most characters a section's name may have, once trimmed: Claude
/// Desktop's limit (PROTO-CORE-API-R76).
const MAX_NAME_CHARS: usize = 200;

pub(super) async fn list(core: &Core, _: Empty) -> Result<Vec<Section>, ApiError> {
    Ok(core.store.sections().await?)
}

/// Makes a section, or answers with the one that has exactly the name
/// given, making none, as Claude Desktop does (PROTO-CORE-API-R76).
pub(super) async fn create(core: &Core, params: SectionCreateParams) -> Result<Section, ApiError> {
    let name = checked_name(&params.name)?;
    let admission = core.admission.clone();
    let section = core
        .store
        .insert_section(name, move |section| {
            // Told before the store takes another call, so no client has an
            // id admission has not: not this one's, nor another that finds
            // this section by its name a moment later. A section found, not
            // made, was told of by the call that made it.
            admission.report(Report::SectionStored {
                section: section.id.clone(),
            });
        })
        .await?;
    Ok(section)
}

/// Renames a section; refuses a name exactly another section's with
/// `invalid_params` (PROTO-CORE-API-R76).
pub(super) async fn rename(core: &Core, params: SectionRenameParams) -> Result<Section, ApiError> {
    let name = checked_name(&params.name)?;
    core.store
        .rename_section(params.id.clone(), name)
        .await
        .map_err(name_taken)?
        .ok_or_else(|| SessionError::NoSection(params.id.clone()).into())
}

/// Removes the section and puts its sessions in no section
/// (PROTO-CORE-API-R39); answers with the section as it was.
pub(super) async fn remove(core: &Core, params: IdParams) -> Result<Section, ApiError> {
    core.sessions
        .remove_section(&params.id)
        .await?
        .ok_or_else(|| SessionError::NoSection(params.id.clone()).into())
}

/// The name as one line, as a session's title is taken. Refuses a name
/// holding a control character other than spacing (PROTO-CORE-API-R60), and
/// one that is then blank or longer than 200 characters
/// (PROTO-CORE-API-R76). Characters, not bytes: a name in an accented or
/// non-Latin script is as long as it reads. Case is kept and compared, so
/// "Work" and "work" are two sections.
fn checked_name(name: &str) -> Result<String, ApiError> {
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
    Ok(name)
}

/// The store refuses a rename to a name another section has as a conflict
/// on the name's key: that is the caller's params, not Core's state, so it
/// answers `invalid_params` (PROTO-CORE-API-R76).
fn name_taken(e: persistence::Error) -> ApiError {
    match e {
        persistence::Error::Conflict(message) => ApiError::invalid_params(message),
        other => other.into(),
    }
}
