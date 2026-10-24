use axum::{
    extract::{Path, Query, State},
    http::header::CONTENT_TYPE,
    response::IntoResponse,
};
use protect_axum::authorities::AuthDetails;
use serde::Deserialize;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::models::Role,
    utils::{
        epg::{EPG_DEFAULT_DAYS, EpgChannel, build_epg, to_xmltv},
        errors::ServiceError,
    },
};

#[derive(Debug, Deserialize)]
pub struct EpgObj {
    days: Option<i64>,
}

/// **XMLTV guide**
///
/// Export the channel schedule as XMLTV, starting today; `days` (1–14, default 3).
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/epg/1?days=7 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_epg(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(obj): Query<EpgObj>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<impl IntoResponse, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await.clone();
    let channel = EpgChannel {
        id: format!("vizrs.{id}"),
        display_name: manager.channel.lock().await.name.clone(),
    };
    let programmes = build_epg(&config, obj.days.unwrap_or(EPG_DEFAULT_DAYS)).await?;

    Ok((
        [(CONTENT_TYPE, "application/xml; charset=utf-8")],
        to_xmltv(&channel, &programmes),
    ))
}
