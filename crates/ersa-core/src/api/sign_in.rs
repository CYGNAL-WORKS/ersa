//! `backend.sign_in` and `backend.cancel_sign_in` (PROTO-CORE-API,
//! Signing in). The sign-in itself is `crate::sign_in`'s.

use std::sync::Arc;

use ersa_protocol::core_api::{BackendSignInParams, Empty, ErrorKind, SignInResult};

use super::ApiError;
use super::handlers::{known_host, unreachable_host};
use crate::admission::SignInOf;
use crate::persistence::LOCAL_HOST_ID;
use crate::sign_in;
use crate::state::Core;

/// Starts the backend's sign-in on the host, or joins the one under way,
/// and answers once it has started (PROTO-CORE-API-R155 to R157).
pub(super) async fn sign_in(
    core: &Arc<Core>,
    params: BackendSignInParams,
) -> Result<SignInResult, ApiError> {
    known_host(core, &params.host_id).await?;
    if params.host_id != LOCAL_HOST_ID {
        return Err(unreachable_host(&params.host_id));
    }
    let Some(adapter) = core.sessions.adapter(&params.name) else {
        return Err(ApiError::new(
            ErrorKind::BackendMissing,
            format!("Ersa has no adapter for the backend {}", params.name),
        ));
    };
    let of = SignInOf {
        host: params.host_id,
        backend: params.name,
    };
    let url = sign_in::begin(core, of, adapter).await?;
    Ok(SignInResult { url })
}

/// Stops the sign-in under way for the backend on the host, and succeeds,
/// doing nothing, when none is: a backend Core has no adapter for, or a
/// host it cannot reach, has none (PROTO-CORE-API-R160).
pub(super) async fn cancel_sign_in(
    core: &Core,
    params: BackendSignInParams,
) -> Result<Empty, ApiError> {
    known_host(core, &params.host_id).await?;
    let of = SignInOf {
        host: params.host_id,
        backend: params.name,
    };
    sign_in::cancel(core, &of).await;
    Ok(Empty {})
}
